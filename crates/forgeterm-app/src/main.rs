// Hide the extra console window in release builds on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod commands;
mod fuzzy;
mod glyphs;
mod keys;
mod palette;
mod terminal_pane;
mod terminal_view;
mod theme;
mod workspace;

use tracing_subscriber::EnvFilter;

/// Default log filter: our crates at info, GPU/windowing internals only when they warn.
const DEFAULT_LOG_FILTER: &str =
    "info,wgpu_hal=error,wgpu_core=warn,iced_wgpu=warn,iced_winit=warn";

fn main() -> iced::Result {
    // GPU defaults, each overridable by the user through the same variable:
    // - low-power adapter: a terminal should not wake a laptop's discrete GPU;
    // - DX12 on Windows: measured ~60-90 MB less memory than Vulkan (docs/TEST_PLAN.md).
    let mut defaults = vec![("WGPU_POWER_PREF", "low")];
    if cfg!(windows) {
        defaults.push(("WGPU_BACKEND", "dx12"));
    }
    for (key, value) in defaults {
        if std::env::var_os(key).is_none() {
            // SAFETY: runs first thing in `main`, before any thread is spawned, so no
            // other thread can be reading the environment concurrently.
            #[allow(unsafe_code)]
            unsafe {
                std::env::set_var(key, value);
            }
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER)),
        )
        .init();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "ForgeTerm starting — Created by Erik van de Blaak"
    );

    iced::application(app::App::boot, app::App::update, app::App::view)
        .title(app::App::title)
        .subscription(app::App::subscription)
        .theme(|_: &app::App| Some(theme::forge_dark()))
        .font(include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf").as_slice())
        .font(include_bytes!("../../../assets/fonts/JetBrainsMono-Bold.ttf").as_slice())
        .font(include_bytes!("../../../assets/fonts/JetBrainsMono-Italic.ttf").as_slice())
        .window(iced::window::Settings {
            size: iced::Size::new(1200.0, 760.0),
            min_size: Some(iced::Size::new(480.0, 300.0)),
            ..Default::default()
        })
        .antialiasing(false)
        .run()
}
