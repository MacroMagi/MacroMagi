//! The settings window.

use egui::{RichText, Ui};
use egui_phosphor::regular as icon;
use magi_core::hotkey::Hotkey;
use magi_core::project::EnginePreference;
use magi_engine::PythonStatus;

use super::widgets::{self, HotkeyAction};
use super::{HotkeyTarget, UiRequest};
use crate::paths::Paths;
use crate::settings::Settings;
use crate::theme;

pub struct SettingsEnv<'a> {
    pub capturing: Option<(&'a HotkeyTarget, &'a Hotkey)>,
    pub python: &'a PythonStatus,
    pub paths: &'a Paths,
}

pub enum SettingsEvent {
    Request(UiRequest),
    /// Try to start Python now and report what happened.
    CheckPython,
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(title.to_uppercase()).size(10.5).color(theme::TEXT_FAINT));
}

fn hotkey_row(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    hotkey: &mut Hotkey,
    target: HotkeyTarget,
    env: &SettingsEnv<'_>,
    events: &mut Vec<SettingsEvent>,
) {
    ui.horizontal(|ui| {
        ui.add_sized([130.0, 20.0], egui::Label::new(label).halign(egui::Align::Min)).on_hover_text(hint);
        let pending = env.capturing.filter(|(active, _)| **active == target).map(|(_, pending)| pending);
        match widgets::hotkey_button(ui, hotkey, pending) {
            HotkeyAction::Capture => events.push(SettingsEvent::Request(UiRequest::CaptureHotkey(target))),
            HotkeyAction::Clear => *hotkey = Hotkey::none(),
            HotkeyAction::None => {}
        }
    });
}

pub fn show(ui: &mut Ui, settings: &mut Settings, env: &SettingsEnv<'_>) -> Vec<SettingsEvent> {
    let mut events = Vec::new();

    section(ui, "Hotkeys");
    hotkey_row(
        ui,
        "Stop everything",
        "Stops the auto clicker and every running macro, whatever has focus.",
        &mut settings.stop_hotkey,
        HotkeyTarget::StopEverything,
        env,
        &mut events,
    );
    hotkey_row(
        ui,
        "Record",
        "Starts and stops recording your input.",
        &mut settings.record_hotkey,
        HotkeyTarget::Record,
        env,
        &mut events,
    );
    ui.label(
        RichText::new("Hotkeys can be any keyboard key, mouse button or gamepad button, alone or combined.")
            .size(11.5)
            .color(theme::TEXT_FAINT),
    );

    section(ui, "Engine");
    widgets::segmented(
        ui,
        &mut settings.engine,
        &[
            (EnginePreference::Auto, "Automatic"),
            (EnginePreference::Python, "Python"),
            (EnginePreference::Native, "Native"),
        ],
    );
    let explanation = match settings.engine {
        EnginePreference::Auto => "Uses Python when it is installed, otherwise the built-in native engine.",
        EnginePreference::Python => "Always uses Python. Needed for Python Script blocks.",
        EnginePreference::Native => "Always uses the built-in Rust engine. Needs nothing installed.",
    };
    ui.label(RichText::new(explanation).size(11.5).color(theme::TEXT_FAINT));
    ui.horizontal(|ui| {
        let (text, color) = match env.python {
            PythonStatus::Running { version, command } => (format!("Python {version} ({command})"), theme::SUCCESS),
            PythonStatus::Unknown => ("Python has not been started yet".to_owned(), theme::TEXT_DIM),
            PythonStatus::Unavailable(reason) => (reason.clone(), theme::WARNING),
        };
        if ui.button("Check").on_hover_text("Start Python now to see whether it works").clicked() {
            events.push(SettingsEvent::CheckPython);
        }
        ui.add(egui::Label::new(RichText::new(text).size(11.5).color(color)).wrap());
    });
    ui.horizontal(|ui| {
        let mut path = settings.python_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        let field = egui::TextEdit::singleline(&mut path)
            .hint_text("Python interpreter (found automatically)")
            .desired_width(190.0);
        if ui.add(field).changed() {
            settings.python_path = (!path.trim().is_empty()).then(|| path.trim().into());
        }
        if ui.button(icon::FOLDER_OPEN).on_hover_text("Choose a Python interpreter").clicked()
            && let Some(file) = rfd::FileDialog::new().set_title("Choose the Python interpreter").pick_file()
        {
            settings.python_path = Some(file);
        }
    });

    section(ui, "Recording");
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut settings.record.keyboard, "Keyboard");
        ui.checkbox(&mut settings.record.mouse_buttons, "Mouse buttons");
        ui.checkbox(&mut settings.record.mouse_moves, "Mouse movement");
        ui.checkbox(&mut settings.record.scroll, "Scroll wheel");
        ui.checkbox(&mut settings.record.gamepad, "Gamepad");
    });
    ui.checkbox(&mut settings.record.trim_start, "Skip the pause before the first input");
    ui.checkbox(&mut settings.auto_insert_recording, "Add a Play Recording block when a recording ends");
    ui.label(
        RichText::new(format!(
            "{}  A recording contains everything you type while it runs, passwords included.",
            icon::WARNING
        ))
        .size(11.5)
        .color(theme::WARNING),
    );

    section(ui, "Window");
    ui.checkbox(&mut settings.always_on_top, "Keep MacroMagi on top of other windows");
    ui.checkbox(&mut settings.hide_from_capture, "Hide MacroMagi from screen captures").on_hover_text(
        "Keeps this window out of what macros see, and out of images you capture for them.\n\
             It also hides the window from screenshots and screen sharing – turn it off to record a tutorial.",
    );
    ui.horizontal(|ui| {
        ui.label("Interface size");
        ui.add(
            egui::Slider::new(&mut settings.ui_scale, 0.75..=1.75)
                .step_by(0.05)
                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
        );
    });

    section(ui, "Files");
    ui.checkbox(&mut settings.include_presets, "Include my presets in saved .magi files")
        .on_hover_text("Bundles your preset library into each macro you save, so they travel with it.");

    section(ui, "Safety");
    ui.checkbox(&mut settings.dry_run, "Dry run: do not send any input").on_hover_text(
        "Macros run normally but nothing is pressed or clicked. The console lists what would have been sent.",
    );

    section(ui, "About");
    ui.label(format!("MacroMagi {}", magi_core::VERSION));
    ui.label(
        RichText::new(format!(
            "Portable: everything MacroMagi saves is in\n{}\nDelete the app's folder and nothing is left behind.",
            env.paths.data_dir.display()
        ))
        .size(11.5)
        .color(theme::TEXT_FAINT),
    );
    events
}
