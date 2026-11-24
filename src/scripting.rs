use crate::{project::Project, scene::Scene};
use anyhow::{Context, Result, ensure};
use chrono::{Datelike, Local, Timelike};
use rhai::{AST, CallFnOptions, Dynamic, Engine, ImmutableString, Map, Module, Scope};
use std::{fs, path::PathBuf, process::Command, sync::{Arc, Mutex}};

pub type SharedScene = Arc<Mutex<Scene>>;

pub struct ScriptRuntime {
    engine: Engine,
    ast: AST,
    scope: Scope<'static>,
}

impl ScriptRuntime {
    pub fn load(project: &Project, scene: SharedScene) -> Result<Self> {
        let config = project
            .manifest
            .scripts
            .as_ref()
            .context("В манифесте не настроена секция scripts")?;
        let script_path = project.root.join(&config.entry);
        let source = fs::read_to_string(&script_path)
            .with_context(|| format!("Не удалось прочитать скрипт {}", script_path.display()))?;

        let mut engine = Engine::new();
        // Scripts are trusted project code and intentionally execute with the
        // same OS permissions as the user running Scene Engine.
        register_api(&mut engine, scene, project.root.clone());
        register_constants(&mut engine);

        let ast = engine
            .compile(&source)
            .map_err(|error| anyhow::anyhow!("Ошибка компиляции скрипта {}: {error}", script_path.display()))?;
        ensure!(
            ast.iter_functions().any(|function| function.name == "update" && function.params.len() == 1),
            "Скрипт {} должен содержать функцию update(dt)",
            script_path.display()
        );

        let mut scope = Scope::new();
        engine
            .run_ast_with_scope(&mut scope, &ast)
            .map_err(|error| anyhow::anyhow!("Ошибка выполнения глобального кода скрипта {}: {error}", script_path.display()))?;

        let mut runtime = Self { engine, ast, scope };
        runtime.call_optional("init")
            .with_context(|| format!("Ошибка выполнения init() в {}", script_path.display()))?;

        Ok(runtime)
    }

    pub fn update(&mut self, dt: f64) -> Result<()> {
        // Do not retain temporary variables declared inside update() on every tick.
        self.engine
            // Lifecycle callbacks may naturally end with an API call such as
            // set_rotation(), whose return value is bool. The engine does not
            // use callback return values, so accept any Rhai value here.
            .call_fn_with_options::<Dynamic>(
                CallFnOptions::new().eval_ast(false),
                &mut self.scope,
                &self.ast,
                "update",
                (dt,),
            )
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!("Ошибка выполнения update(dt): {error}"))
    }

    fn call_optional(&mut self, name: &str) -> Result<()> {
        if !self.ast.iter_functions().any(|function| function.name == name && function.params.is_empty()) {
            return Ok(());
        }

        // Keep state variables introduced by init() in the script Scope.
        self.engine
            // `init()` commonly ends with a scene API call returning bool.
            // Ignore its result instead of requiring the script to return unit.
            .call_fn_with_options::<Dynamic>(
                CallFnOptions::new().eval_ast(false).rewind_scope(false),
                &mut self.scope,
                &self.ast,
                name,
                (),
            )
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!("{error}"))
    }
}

fn register_constants(engine: &mut Engine) {
    // Engine-level module constants are visible both in the global script body
    // and inside Rhai functions. Scope constants alone are not reliably visible
    // from script-defined functions because those functions are pure.
    let mut constants = Module::new();
    constants.set_var("PI", std::f64::consts::PI);
    constants.set_var("TAU", std::f64::consts::TAU);
    engine.register_global_module(constants.into());
}

fn register_api(engine: &mut Engine, scene: SharedScene, project_root: PathBuf) {
    engine.register_fn("set_position", {
        let scene = Arc::clone(&scene);
        move |id: ImmutableString, x: f64, y: f64| -> bool {
            scene.lock().map(|mut scene| scene.set_position(id.as_str(), x as f32, y as f32)).unwrap_or(false)
        }
    });

    engine.register_fn("move_by", {
        let scene = Arc::clone(&scene);
        move |id: ImmutableString, x: f64, y: f64| -> bool {
            scene.lock().map(|mut scene| scene.move_by(id.as_str(), x as f32, y as f32)).unwrap_or(false)
        }
    });

    engine.register_fn("set_size", {
        let scene = Arc::clone(&scene);
        move |id: ImmutableString, width: f64, height: f64| -> bool {
            scene.lock().map(|mut scene| scene.set_size(id.as_str(), width as f32, height as f32)).unwrap_or(false)
        }
    });

    engine.register_fn("set_rotation", {
        let scene = Arc::clone(&scene);
        move |id: ImmutableString, angle: f64| -> bool {
            scene.lock().map(|mut scene| scene.set_rotation(id.as_str(), angle as f32)).unwrap_or(false)
        }
    });

    engine.register_fn("set_opacity", {
        let scene = Arc::clone(&scene);
        move |id: ImmutableString, opacity: f64| -> bool {
            scene.lock().map(|mut scene| scene.set_opacity(id.as_str(), opacity as f32)).unwrap_or(false)
        }
    });

    engine.register_fn("set_visible", {
        let scene = Arc::clone(&scene);
        move |id: ImmutableString, visible: bool| -> bool {
            scene.lock().map(|mut scene| scene.set_visible(id.as_str(), visible)).unwrap_or(false)
        }
    });

    engine.register_fn("set_z_index", {
        let scene = Arc::clone(&scene);
        move |id: ImmutableString, z_index: i64| -> bool {
            if z_index < i32::MIN as i64 || z_index > i32::MAX as i64 {
                return false;
            }
            scene.lock().map(|mut scene| scene.set_z_index(id.as_str(), z_index as i32)).unwrap_or(false)
        }
    });


    engine.register_fn("set_text", {
        let scene = Arc::clone(&scene);
        move |id: ImmutableString, text: ImmutableString| -> bool {
            scene.lock().map(|mut scene| scene.set_text(id.as_str(), text.to_string())).unwrap_or(false)
        }
    });

    engine.register_fn("set_frame", move |id: ImmutableString, frame: i64| -> bool {
        if frame < 0 || frame > u32::MAX as i64 {
            return false;
        }
        scene.lock().map(|mut scene| scene.set_frame(id.as_str(), frame as u32)).unwrap_or(false)
    });

    // Execute a command through Bash. This is deliberately not sandboxed:
    // projects can access anything the current user can access.
    engine.register_fn("exec", move |command: ImmutableString| -> Map {
        execute_bash(command.as_str(), &project_root)
    });

    engine.register_fn("local_time", || -> Map {
        let now = Local::now();
        let mut time = Map::new();
        time.insert("year".into(), Dynamic::from(now.year() as i64));
        time.insert("month".into(), Dynamic::from(now.month() as i64));
        time.insert("day".into(), Dynamic::from(now.day() as i64));
        time.insert("hour".into(), Dynamic::from(now.hour() as i64));
        time.insert("minute".into(), Dynamic::from(now.minute() as i64));
        time.insert("second".into(), Dynamic::from(now.second() as i64));
        time.insert("millisecond".into(), Dynamic::from(now.timestamp_subsec_millis() as i64));
        time
    });
}

fn execute_bash(command: &str, working_directory: &std::path::Path) -> Map {
    let mut result = Map::new();
    match Command::new("bash")
        .arg("-c")
        .arg(command)
        .current_dir(working_directory)
        .output()
    {
        Ok(output) => {
            result.insert("success".into(), Dynamic::from(output.status.success()));
            result.insert("code".into(), Dynamic::from(output.status.code().unwrap_or(-1) as i64));
            result.insert("stdout".into(), Dynamic::from(String::from_utf8_lossy(&output.stdout).into_owned()));
            result.insert("stderr".into(), Dynamic::from(String::from_utf8_lossy(&output.stderr).into_owned()));
        }
        Err(error) => {
            result.insert("success".into(), Dynamic::from(false));
            result.insert("code".into(), Dynamic::from(-1_i64));
            result.insert("stdout".into(), Dynamic::from(String::new()));
            result.insert("stderr".into(), Dynamic::from(error.to_string()));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_constants_are_available_inside_script_functions() {
        let mut engine = Engine::new();
        register_constants(&mut engine);

        let ast = engine
            .compile(
                r#"
                    fn get_pi() { PI }
                    fn get_tau() { TAU }
                "#,
            )
            .expect("script with PI and TAU should compile");
        let mut scope = Scope::new();
        engine
            .run_ast_with_scope(&mut scope, &ast)
            .expect("global script code should execute");

        let pi = engine
            .call_fn_with_options::<f64>(
                CallFnOptions::new().eval_ast(false),
                &mut scope,
                &ast,
                "get_pi",
                (),
            )
            .expect("PI should be visible inside script functions");
        let tau = engine
            .call_fn_with_options::<f64>(
                CallFnOptions::new().eval_ast(false),
                &mut scope,
                &ast,
                "get_tau",
                (),
            )
            .expect("TAU should be visible inside script functions");

        assert!((pi - std::f64::consts::PI).abs() < f64::EPSILON);
        assert!((tau - std::f64::consts::TAU).abs() < f64::EPSILON);
    }

    #[test]
    fn lifecycle_callbacks_accept_non_unit_return_values() {
        let mut engine = Engine::new();
        engine.register_fn("returns_bool", || true);

        let ast = engine
            .compile(
                r#"
                    fn init() { returns_bool() }
                    fn update(dt) { returns_bool() }
                "#,
            )
            .expect("script with bool-returning callbacks should compile");
        let mut scope = Scope::new();
        engine
            .run_ast_with_scope(&mut scope, &ast)
            .expect("global script code should execute");

        engine
            .call_fn_with_options::<Dynamic>(
                CallFnOptions::new().eval_ast(false).rewind_scope(false),
                &mut scope,
                &ast,
                "init",
                (),
            )
            .expect("init() should accept a bool return value");

        engine
            .call_fn_with_options::<Dynamic>(
                CallFnOptions::new().eval_ast(false),
                &mut scope,
                &ast,
                "update",
                (1.0_f64,),
            )
            .expect("update(dt) should accept a bool return value");
    }

    #[test]
    fn lifecycle_callbacks_also_accept_unit_return_values() {
        let mut engine = Engine::new();
        let ast = engine
            .compile(
                r#"
                    fn init() { }
                    fn update(dt) { }
                "#,
            )
            .expect("script with unit-returning callbacks should compile");
        let mut scope = Scope::new();
        engine
            .run_ast_with_scope(&mut scope, &ast)
            .expect("global script code should execute");

        engine
            .call_fn_with_options::<Dynamic>(
                CallFnOptions::new().eval_ast(false).rewind_scope(false),
                &mut scope,
                &ast,
                "init",
                (),
            )
            .expect("init() should accept unit return value");

        engine
            .call_fn_with_options::<Dynamic>(
                CallFnOptions::new().eval_ast(false),
                &mut scope,
                &ast,
                "update",
                (1.0_f64,),
            )
            .expect("update(dt) should accept unit return value");
    }

    #[test]
    fn bash_execution_returns_stdout_stderr_and_exit_code() {
        let root = std::env::current_dir().unwrap();
        let result = execute_bash("printf 'hello'; printf 'warning' >&2; exit 7", &root);
        assert!(!result["success"].as_bool().unwrap());
        assert_eq!(result["code"].as_int().unwrap(), 7);
        assert_eq!(result["stdout"].clone().into_string().unwrap(), "hello");
        assert_eq!(result["stderr"].clone().into_string().unwrap(), "warning");
    }

}
