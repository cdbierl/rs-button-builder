mod app;
mod cli;
mod fonts;
mod layout_config;
mod settings;
mod terminal;
mod theme;

use app::ui;
use std::env;
use std::io::ErrorKind;
use std::process::ExitCode;

/// Initializes logging and configuration, then returns an appropriate process status.
fn main() -> ExitCode {
    let dotenv_result = dotenvy::dotenv();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    match dotenv_result {
        Ok(path) => log::info!("Loaded environment from {}", path.display()),
        Err(dotenvy::Error::Io(error)) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            log::error!("could not load .env: {error}");
            return ExitCode::FAILURE;
        }
    }

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            log::error!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let action = cli::parse_args(env::args().skip(1))?;

    match action {
        cli::Action::Help => print!("{}", cli::USAGE),
        cli::Action::Run { config } => {
            let mut app_settings = settings::SettingsStore::load();
            let (path, settings) = app_settings.startup_layout(config)?;
            let options = eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_title(format!("Button Builder - {}", settings.display_name()))
                    .with_inner_size([1200.0, 760.0])
                    .with_min_inner_size([850.0, 500.0]),
                ..Default::default()
            };
            let result = eframe::run_native(
                "Button Builder",
                options,
                Box::new(move |cc| {
                    Ok(Box::new(
                        app::ButtonBuilder::new(&cc.egui_ctx, path, settings)
                            .with_settings(app_settings),
                    ))
                }),
            );
            terminal::finish_cleanup();
            result.map_err(|error| error.to_string())?;
        }
    }

    Ok(())
}
