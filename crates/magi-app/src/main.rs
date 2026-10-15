//! MacroMagi – an auto clicker and node-based macro editor.
//!
//! This crate is the desktop application. It contains the user interface and
//! the glue between it and the engine; everything else lives in the library
//! crates (see `docs/ARCHITECTURE.md`).

// Release builds on Windows are GUI programs: no console window pops up.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod background;
mod cli;
mod controller;
mod displays;
mod document;
mod examples;
mod history;
mod icon;
mod paths;
mod presets;
mod settings;
mod theme;
mod ui;

use std::process::ExitCode;
use std::sync::{Arc, OnceLock};

use app::{MagiApp, StartupOptions};
use paths::Paths;
use settings::Settings;

const DEFAULT_SIZE: [f32; 2] = [440.0, 660.0];
pub const MIN_SIZE: [f32; 2] = [360.0, 420.0];

/// Window size forced from the command line (`--size`), for screenshots.
pub static WINDOW_SIZE_OVERRIDE: OnceLock<Option<[f32; 2]>> = OnceLock::new();

fn main() -> ExitCode {
    magi_platform::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        // A GUI program has no console of its own; borrow the one we were started from.
        magi_platform::attach_parent_console();
    }
    match cli::parse(&args) {
        Ok(cli::Command::Help) => {
            print!("{}", cli::HELP);
            ExitCode::SUCCESS
        }
        Ok(cli::Command::Version) => {
            println!("MacroMagi {}", magi_core::VERSION);
            ExitCode::SUCCESS
        }
        Ok(cli::Command::Nodes) => cli::nodes(),
        Ok(cli::Command::Example { index, file }) => cli::example(index, &file),
        Ok(cli::Command::Run { file, engine, dry_run }) => cli::run(&file, engine, dry_run),
        Ok(cli::Command::Gui(options)) => gui(options),
        Err(message) => {
            eprintln!("{message}\n\n{}", cli::HELP);
            ExitCode::FAILURE
        }
    }
}

fn gui(options: StartupOptions) -> ExitCode {
    let paths = Paths::detect();
    let settings = Settings::load(&paths.settings_file());

    let size = WINDOW_SIZE_OVERRIDE.get().copied().flatten().or(settings.window_size).unwrap_or(DEFAULT_SIZE);
    let viewport = egui::ViewportBuilder::default()
        .with_title("MacroMagi")
        .with_app_id("macromagi")
        .with_inner_size(size)
        .with_min_inner_size(MIN_SIZE)
        .with_icon(Arc::new(icon::window_icon()));
    let native_options = eframe::NativeOptions {
        viewport,
        // MacroMagi is portable: eframe must not store window state in the user profile.
        persist_window: false,
        centered: true,
        ..Default::default()
    };

    let result = eframe::run_native(
        "MacroMagi",
        native_options,
        Box::new(move |cc| Ok(Box::new(MagiApp::new(cc, paths, settings, options)))),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("MacroMagi could not start: {error}");
            ExitCode::FAILURE
        }
    }
}
