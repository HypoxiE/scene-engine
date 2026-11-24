mod app;
mod project;
mod renderer;
mod scene;
mod scripting;

use anyhow::{bail, Context, Result};
use std::{env, ffi::OsStr, path::PathBuf};
use winit::{event_loop::{ControlFlow, EventLoop}};

fn main() {
    if let Err(error) = run() {
        eprintln!("Ошибка: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let project_path = project_path_from_args()?;

    let project = project::Project::load(&project_path)
        .with_context(|| format!("Не удалось загрузить проект {}", project_path.display()))?;

    println!("Проект: {}", project.manifest.name);
    println!(
        "Сцена: {}x{}, ресурсов: {}, объектов: {}",
        project.manifest.canvas.width,
        project.manifest.canvas.height,
        project.manifest.assets.len(),
        project.manifest.nodes.len()
    );

    let event_loop = EventLoop::new().context("Не удалось создать цикл событий winit")?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = app::App::new(project);
    event_loop
        .run_app(&mut app)
        .context("Ошибка цикла событий winit")?;

    Ok(())
}

fn project_path_from_args() -> Result<PathBuf> {
    let mut args = env::args_os();
    let executable = args.next().unwrap_or_default();

    let Some(argument) = args.next() else {
        return Ok(PathBuf::from("examples/demo"));
    };

    if argument == OsStr::new("--help") || argument == OsStr::new("-h") {
        println!(
            "Использование: {} [ДИРЕКТОРИЯ_ПРОЕКТА]\n\nДиректория должна содержать manifest.yaml.\nЕсли путь не указан, используется examples/demo.",
            PathBuf::from(executable).display(),
        );
        std::process::exit(0);
    }

    if args.next().is_some() {
        bail!("Ожидается один аргумент: путь к директории проекта. Используйте --help для справки.");
    }

    Ok(PathBuf::from(argument))
}
