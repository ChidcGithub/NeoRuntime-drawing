//! Opt-in native presentation diagnostics; never writes protocol stdout or pixel data.
use log::{Level, LevelFilter, Log, Metadata, Record};

struct PresentationLogger;
static LOGGER: PresentationLogger = PresentationLogger;

fn relevant(target: &str, level: Level) -> bool {
    (target == "wgpu_core::device::resource" && level <= Level::Debug)
        || ([
            "egui_wgpu",
            "wgpu_hal",
            "winit",
            "egui_glow",
            "eframe",
            "glutin",
        ]
        .iter()
        .any(|prefix| {
            target == *prefix
                || target
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with("::"))
        }) && level <= Level::Warn)
}

impl Log for PresentationLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        relevant(metadata.target(), metadata.level())
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        // This is the real SurfaceConfiguration logged by wgpu-core, not a
        // guessed alpha mode or a second surface created solely for inspection.
        if record.level() <= Level::Warn || message.starts_with("configuring surface with ") {
            eprintln!(
                "[render] {} {}: {}",
                record.level(),
                record.target(),
                message
            );
        }
    }

    fn flush(&self) {}
}

pub(crate) fn install_from_env() -> bool {
    if std::env::var("NEO_DRAW_RENDER_DIAGNOSTICS").as_deref() != Ok("1") {
        return false;
    }
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    } else {
        eprintln!("[render] Logger already installed; surface logs depend on its filter");
    }
    true
}

pub(crate) fn select_renderer(
    mode: crate::AppMode,
    value: Option<&str>,
) -> Result<eframe::Renderer, &'static str> {
    match value {
        // Windows transparent presentation was verified with Glow; keep the
        // opaque blackboard and other platforms on their existing wgpu path.
        None if cfg!(windows) && mode == crate::AppMode::Drawing => Ok(eframe::Renderer::Glow),
        None | Some("wgpu") => Ok(eframe::Renderer::Wgpu),
        Some("glow") => Ok(eframe::Renderer::Glow),
        Some(_) => {
            Err("NEO_DRAW_RENDERER 仅支持 wgpu 或 glow；删除该环境变量可恢复平台/应用默认后端")
        }
    }
}

#[cfg(test)]
#[path = "render_diagnostics_tests.rs"]
mod tests;
