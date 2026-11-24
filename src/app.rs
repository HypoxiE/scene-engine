use crate::{
    project::Project,
    renderer::GpuRenderer,
    scene::Scene,
    scripting::{ScriptRuntime, SharedScene},
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow},
    window::{Window, WindowId},
};

pub struct App {
    project: Option<Project>,
    scene: SharedScene,
    scripts: Option<ScriptRuntime>,
    script_interval: Option<Duration>,
    last_script_tick: Instant,
    next_script_tick: Instant,
    window: Option<Arc<Window>>,
    renderer: Option<GpuRenderer>,
}

impl App {
    pub fn new(project: Project) -> Self {
        let scene = Arc::new(Mutex::new(Scene::from_manifest(&project.manifest)));
        Self {
            project: Some(project),
            scene,
            scripts: None,
            script_interval: None,
            last_script_tick: Instant::now(),
            next_script_tick: Instant::now(),
            window: None,
            renderer: None,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let Some(project) = self.project.take() else {
            eprintln!("Ошибка: проект уже был передан рендереру");
            event_loop.exit();
            return;
        };

        if let Some(config) = project.manifest.scripts.as_ref() {
            let tick_interval = Duration::from_secs_f64(1.0 / config.tick_rate as f64);
            match ScriptRuntime::load(&project, Arc::clone(&self.scene)) {
                Ok(runtime) => {
                    self.scripts = Some(runtime);
                    self.script_interval = Some(tick_interval);
                    self.last_script_tick = Instant::now();
                    self.next_script_tick = self.last_script_tick + tick_interval;
                    println!(
                        "Скрипт загружен: {} ({} тиков/с)",
                        config.entry.display(),
                        config.tick_rate
                    );
                }
                Err(error) => {
                    eprintln!("Не удалось загрузить скриптовую систему: {error:#}");
                    event_loop.exit();
                    return;
                }
            }
        }

        let attributes = Window::default_attributes()
            .with_title(project.manifest.name.clone())
            .with_inner_size(PhysicalSize::new(1280, 720))
            .with_visible(false);

        // На Wayland это задаёт app_id, на X11 — WM_CLASS.
        #[cfg(target_os = "linux")]
        let attributes = {
            use winit::platform::{
                wayland::WindowAttributesExtWayland,
                x11::WindowAttributesExtX11,
            };

            let attributes = WindowAttributesExtWayland::with_name(
                attributes,
                "scene_engine",
                "scene_engine",
            );

            WindowAttributesExtX11::with_name(
                attributes,
                "scene_engine",
                "scene_engine",
            )
        };

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("Не удалось создать окно: {error}");
                event_loop.exit();
                return;
            }
        };

        let display_handle = event_loop.owned_display_handle();
        let scene = match self.scene.lock() {
            Ok(scene) => scene,
            Err(error) => {
                eprintln!("Не удалось получить доступ к сцене: {error}");
                event_loop.exit();
                return;
            }
        };
        match pollster::block_on(GpuRenderer::new(
            window.clone(),
            display_handle,
            &project,
            &scene,
        )) {
            Ok(renderer) => {
                drop(scene);
                window.set_visible(true);
                window.request_redraw();
                self.window = Some(window);
                self.renderer = Some(renderer);
            }
            Err(error) => {
                eprintln!("Не удалось инициализировать графический движок: {error:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    match self.scene.lock() {
                        Ok(scene) => renderer.resize(size, &scene),
                        Err(error) => eprintln!("Не удалось получить доступ к сцене: {error}"),
                    }
                }
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(renderer) = self.renderer.as_mut() {
                    match self.scene.lock() {
                        Ok(scene) => renderer.resize(window.inner_size(), &scene),
                        Err(error) => eprintln!("Не удалось получить доступ к сцене: {error}"),
                    }
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.renderer.as_mut() {
                    let render_result = match self.scene.lock() {
                        Ok(scene) => renderer.render(&scene),
                        Err(error) => {
                            eprintln!("Не удалось получить доступ к сцене: {error}");
                            event_loop.exit();
                            return;
                        }
                    };

                    match render_result {
                        Ok(true) => window.request_redraw(),
                        Ok(false) => {}
                        Err(error) => {
                            eprintln!("Ошибка рендеринга: {error:#}");
                            event_loop.exit();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(interval), Some(window)) = (self.script_interval, self.window.as_ref()) else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };

        let now = Instant::now();
        if now >= self.next_script_tick {
            let dt = now.duration_since(self.last_script_tick).as_secs_f64().min(0.25);
            self.last_script_tick = now;
            self.next_script_tick = now + interval;

            let update_error = self
                .scripts
                .as_mut()
                .and_then(|runtime| runtime.update(dt).err());
            if let Some(error) = update_error {
                eprintln!("Скрипт остановлен из-за ошибки: {error:#}");
                self.scripts = None;
                self.script_interval = None;
                event_loop.set_control_flow(ControlFlow::Wait);
                window.request_redraw();
                return;
            }

            window.request_redraw();
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_script_tick));
    }
}
