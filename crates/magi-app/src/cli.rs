//! Command-line use: running macros without the window, and inspecting node definitions.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use magi_core::geom::{Point, Rect};
use magi_core::host::{Frame, Host, HostError};
use magi_core::hotkey::{HotkeyEdge, HotkeyTracker};
use magi_core::input::InputEvent;
use magi_core::nodes::Registry;
use magi_core::project::{EnginePreference, Project};
use magi_engine::{LogLevel, Monitor, Program, PythonConfig, PythonRuntime, Session};
use magi_platform::{InputHub, SystemHost};

use crate::app::StartupOptions;
use crate::paths::Paths;
use crate::settings::{Settings, Tab};

pub const HELP: &str = "\
MacroMagi – auto clicker and node-based macro editor

USAGE
    macromagi [FILE.magi]              Open the editor, optionally with a macro
    macromagi run FILE.magi [OPTIONS]  Run a macro without opening the window
    macromagi nodes                    Print every node definition as JSON
    macromagi example N FILE.magi      Save built-in example number N (1, 2, …) as a file
    macromagi --version
    macromagi --help

OPTIONS FOR run
    --engine auto|python|native   Engine to use (default: the macro's own choice, then the app setting)
    --dry-run                     Do not send any input; print what would be sent

A running macro stops when it reaches its end, or when you press the
stop-everything hotkey (F8 unless changed in Settings).
";

pub enum Command {
    Gui(StartupOptions),
    Run {
        file: PathBuf,
        engine: Option<EnginePreference>,
        dry_run: bool,
    },
    Nodes,
    /// Write a built-in example macro to a file.
    Example {
        index: usize,
        file: PathBuf,
    },
    Help,
    Version,
}

pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut options = StartupOptions {
        open: None,
        example: None,
        tab: None,
        screenshot: None,
        show_cards: false,
        show_targets: false,
        select_kind: None,
        settings_page: None,
    };
    let mut positional: Vec<&str> = Vec::new();
    let mut engine = None;
    let mut dry_run = false;
    let mut window_size = None;

    let mut iter = args.iter().map(String::as_str);
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| iter.next().ok_or_else(|| format!("{name} needs a value"));
        match arg {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--engine" => {
                engine = Some(match value("--engine")? {
                    "auto" => EnginePreference::Auto,
                    "python" => EnginePreference::Python,
                    "native" => EnginePreference::Native,
                    other => return Err(format!("unknown engine `{other}` (use auto, python or native)")),
                });
            }
            "--dry-run" => dry_run = true,
            // The options below exist for producing documentation screenshots.
            "--screenshot" => options.screenshot = Some(PathBuf::from(value("--screenshot")?)),
            "--example" => {
                let index = value("--example")?.parse().map_err(|_| "--example needs a number".to_owned())?;
                options.example = Some(index);
            }
            "--tab" => {
                options.tab = Some(match value("--tab")? {
                    "clicker" => Tab::AutoClicker,
                    "macro" => Tab::Macro,
                    other => return Err(format!("unknown tab `{other}` (use clicker or macro)")),
                });
            }
            "--cards" => options.show_cards = true,
            "--targets" => options.show_targets = true,
            "--select" => options.select_kind = Some(value("--select")?.to_owned()),
            "--settings" => {
                let page = value("--settings")?.to_lowercase();
                options.settings_page = Some(
                    crate::settings::SettingsPage::ALL
                        .into_iter()
                        .find(|p| p.label().to_lowercase() == page)
                        .ok_or_else(|| format!("unknown settings page `{page}`"))?,
                );
            }
            "--size" => {
                let text = value("--size")?;
                let parsed = text.split_once('x').and_then(|(w, h)| Some([w.parse().ok()?, h.parse().ok()?]));
                window_size = Some(parsed.ok_or_else(|| "--size needs WIDTHxHEIGHT, e.g. 1100x720".to_owned())?);
            }
            flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}`")),
            other => positional.push(other),
        }
    }
    crate::WINDOW_SIZE_OVERRIDE.set(window_size).ok();

    match positional.as_slice() {
        ["run", file] => Ok(Command::Run { file: PathBuf::from(file), engine, dry_run }),
        ["run", ..] => Err("usage: macromagi run FILE.magi".to_owned()),
        ["nodes"] => Ok(Command::Nodes),
        ["example", number, file] => match number.parse::<usize>() {
            Ok(n) if (1..=crate::examples::NAMES.len()).contains(&n) => {
                Ok(Command::Example { index: n - 1, file: PathBuf::from(file) })
            }
            _ => Err(format!("there are examples 1 to {}", crate::examples::NAMES.len())),
        },
        ["example", ..] => Err("usage: macromagi example N FILE.magi".to_owned()),
        [] => Ok(Command::Gui(options)),
        [file] => {
            options.open = Some(PathBuf::from(file));
            Ok(Command::Gui(options))
        }
        _ => Err("too many arguments".to_owned()),
    }
}

/// Prints the node definitions: the machine-readable description of every
/// block, as used by the editor and (in future) by add-on tooling.
pub fn nodes() -> ExitCode {
    let registry = Registry::builtin();
    let defs: Vec<_> = registry.iter().map(|def| &**def).collect();
    match serde_json::to_string_pretty(&defs) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Saves one of the built-in example macros as a `.magi` file.
pub fn example(index: usize, file: &Path) -> ExitCode {
    let project = crate::examples::build(index, &Registry::builtin());
    match project.save(file) {
        Ok(()) => {
            println!("Saved “{}” to {}", crate::examples::NAMES[index], file.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Could not save {}: {error}", file.display());
            ExitCode::FAILURE
        }
    }
}

/// A host for `--dry-run`: reads the real screen and pointer, sends nothing.
struct PrintingHost {
    real: SystemHost,
}

impl Host for PrintingHost {
    fn send(&self, event: &InputEvent) -> Result<(), HostError> {
        println!("would {event}");
        Ok(())
    }

    fn cursor(&self) -> Result<Point, HostError> {
        self.real.cursor()
    }

    fn capture(&self, region: Option<Rect>) -> Result<Frame, HostError> {
        self.real.capture(region)
    }
}

pub fn run(file: &Path, engine: Option<EnginePreference>, dry_run: bool) -> ExitCode {
    let paths = Paths::detect();
    let settings = Settings::load(&paths.settings_file());
    let project = match Project::load(file) {
        Ok(project) => project,
        Err(error) => {
            eprintln!("Could not open {}: {error}", file.display());
            return ExitCode::FAILURE;
        }
    };

    let registry = Arc::new(Registry::builtin());
    let program = Arc::new(Program::from_project(&project, registry));
    let monitor = Arc::new(Monitor::new());
    let host: Arc<dyn Host> =
        if dry_run { Arc::new(PrintingHost { real: SystemHost::new() }) } else { Arc::new(SystemHost::new()) };

    let preference = engine.unwrap_or(match project.engine {
        EnginePreference::Auto => settings.engine,
        chosen => chosen,
    });
    let native = || Session::native(Arc::clone(&program), Arc::clone(&host), Arc::clone(&monitor));
    let python = || {
        let config = PythonConfig {
            interpreter: settings.python_path.clone(),
            app_dir: paths.app_dir.clone(),
            data_dir: paths.data_dir.clone(),
        };
        let runtime = Arc::new(PythonRuntime::new(config, Arc::clone(&monitor)));
        Session::python(Arc::clone(&program), Arc::clone(&host), Arc::clone(&monitor), runtime)
    };
    let session = match preference {
        EnginePreference::Native => native(),
        EnginePreference::Python => match python() {
            Ok(session) => session,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        },
        EnginePreference::Auto => python().unwrap_or_else(|error| {
            eprintln!("{error} Using the native engine instead.");
            native()
        }),
    };
    println!(
        "Running {} on the {} engine. Stop with {}.",
        file.display(),
        session.engine().label(),
        settings.stop_hotkey.label()
    );

    match session.start_all() {
        Ok(0) => {
            eprintln!("This macro has no Start block that runs on start.");
            return ExitCode::FAILURE;
        }
        Ok(_) => {}
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    }

    // The stop hotkey works here just as it does in the app.
    let events = InputHub::start().subscribe();
    let mut tracker = HotkeyTracker::new();
    tracker.set_bindings(vec![(settings.stop_hotkey.clone(), ())]);

    let mut printed = 0;
    let mut failed = false;
    loop {
        while let Ok(observed) = events.try_recv() {
            let pressed = tracker.feed(&observed.event).iter().any(|(_, edge)| *edge == HotkeyEdge::Pressed);
            if pressed && !observed.synthetic {
                println!("Stopping.");
                session.stop_all();
            }
        }
        for entry in monitor.logs_from(printed) {
            printed = entry.seq + 1;
            match entry.level {
                LogLevel::Info => println!("{}", entry.message),
                LogLevel::Warning => eprintln!("warning: {}", entry.message),
                LogLevel::Error => {
                    failed = true;
                    eprintln!("error: {}", entry.message);
                }
            }
        }
        if session.wait_idle(Some(Duration::from_millis(40))) && monitor.logs_from(printed).is_empty() {
            break;
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn commands_are_recognised() {
        assert!(matches!(parse(&args(&[])), Ok(Command::Gui(_))));
        assert!(matches!(parse(&args(&["--help"])), Ok(Command::Help)));
        assert!(matches!(parse(&args(&["nodes"])), Ok(Command::Nodes)));
        assert!(matches!(parse(&args(&["example", "2", "x.magi"])), Ok(Command::Example { index: 1, .. })));
        assert!(parse(&args(&["example", "99", "x.magi"])).is_err());
        match parse(&args(&["run", "a.magi", "--engine", "native", "--dry-run"])) {
            Ok(Command::Run { file, engine, dry_run }) => {
                assert_eq!(file, PathBuf::from("a.magi"));
                assert_eq!(engine, Some(EnginePreference::Native));
                assert!(dry_run);
            }
            _ => panic!("expected a run command"),
        }
        match parse(&args(&["my.magi"])) {
            Ok(Command::Gui(options)) => assert_eq!(options.open, Some(PathBuf::from("my.magi"))),
            _ => panic!("expected the editor to open a file"),
        }
    }

    #[test]
    fn mistakes_are_explained() {
        assert!(parse(&args(&["--engine"])).is_err());
        assert!(parse(&args(&["--engine", "cobol"])).is_err());
        assert!(parse(&args(&["--frobnicate"])).is_err());
        assert!(parse(&args(&["run"])).is_err());
    }
}
