//! The Auto Clicker tab: a compact form over [`AutoClickerConfig`].

use egui::{Color32, DragValue, RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use magi_core::autoclicker::{AutoClickerConfig, ClickLimit, ClickPosition, ClickTarget};
use magi_core::hotkey::Hotkey;
use magi_core::input::{Key, MouseButton};
use magi_core::nodes::flow::StartMode;
use magi_core::preset::{Preset, PresetBody};

use super::widgets::{self, HotkeyAction};
use super::{HotkeyTarget, SnipMode, SnipTarget, UiRequest};
use crate::presets::PresetLibrary;
use crate::theme;

/// Things the tab asks the app to do.
pub enum ClickerEvent {
    Toggle,
    /// Open the equivalent node graph in the Macro tab.
    OpenAsMacro,
    Request(UiRequest),
    Toast(String),
}

#[derive(Default)]
pub struct ClickerTab {
    /// Name being typed for a new preset; `Some` while the save row is open.
    new_preset: Option<String>,
}

pub struct ClickerEnv<'a> {
    pub running: bool,
    /// How many clicks the current (or last) run has made.
    pub clicks: u64,
    pub error: Option<String>,
    pub capturing: Option<(&'a HotkeyTarget, &'a Hotkey)>,
    pub presets: &'a mut PresetLibrary,
}

fn card(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme::BG_RAISED)
        .stroke(egui::Stroke::new(1.0, theme::STROKE))
        .corner_radius(theme::RADIUS_LARGE)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title.to_uppercase()).size(10.5).color(theme::TEXT_FAINT));
            ui.add_space(1.0);
            body(ui);
        });
}

impl ClickerTab {
    pub fn show(&mut self, ui: &mut Ui, config: &mut AutoClickerConfig, env: &mut ClickerEnv<'_>) -> Vec<ClickerEvent> {
        let mut events = Vec::new();

        // The start button stays at the bottom, whatever the window height.
        egui::Panel::bottom("clicker-start")
            .show_separator_line(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 12)))
            .show(ui, |ui| self.footer(ui, config, env, &mut events));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 14, top: 12, bottom: 0 }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.add_enabled_ui(!env.running, |ui| self.form(ui, config, env, &mut events));
                });
            });
        events
    }

    fn form(
        &mut self,
        ui: &mut Ui,
        config: &mut AutoClickerConfig,
        env: &mut ClickerEnv<'_>,
        events: &mut Vec<ClickerEvent>,
    ) {
        card(ui, "Click", |ui| {
            let mut uses_mouse = matches!(config.target, ClickTarget::Mouse { .. });
            ui.horizontal(|ui| {
                if widgets::segmented(ui, &mut uses_mouse, &[(true, "Mouse"), (false, "Keyboard")]) {
                    config.target = if uses_mouse {
                        ClickTarget::Mouse { button: MouseButton::Left }
                    } else {
                        ClickTarget::Key { key: Key::Space }
                    };
                }
                match &mut config.target {
                    ClickTarget::Mouse { button } => {
                        egui::ComboBox::from_id_salt("clicker-button")
                            .width(96.0)
                            .selected_text(button.label())
                            .show_ui(ui, |ui| {
                                for &option in MouseButton::ALL {
                                    ui.selectable_value(button, option, option.label());
                                }
                            });
                    }
                    ClickTarget::Key { key } => {
                        widgets::key_picker(ui, "clicker-key", key);
                    }
                }
            });
            if uses_mouse {
                widgets::segmented(ui, &mut config.clicks, &[(1, "Single"), (2, "Double"), (3, "Triple")]);
            }
        });

        card(ui, "Speed", |ui| {
            egui::Grid::new("clicker-speed").num_columns(2).spacing(vec2(12.0, 7.0)).show(ui, |ui| {
                ui.label("Every");
                ui.horizontal(|ui| {
                    widgets::duration_edit(ui, &mut config.interval_ms);
                    ui.label(
                        RichText::new(widgets::format_rate(config.clicks_per_second()))
                            .size(11.5)
                            .color(theme::TEXT_FAINT),
                    );
                });
                ui.end_row();

                ui.label("Random ±")
                    .on_hover_text("Vary each interval by up to this much, so the rhythm is less mechanical.");
                widgets::duration_edit(ui, &mut config.jitter_ms);
                ui.end_row();

                ui.label("Hold for")
                    .on_hover_text("How long each click stays pressed. Some games ignore clicks that are too short.");
                widgets::duration_edit(ui, &mut config.hold_ms);
                ui.end_row();
            });
        });

        card(ui, "Repeat", |ui| {
            let mut limited = matches!(config.limit, ClickLimit::Count { .. });
            ui.horizontal(|ui| {
                if widgets::segmented(ui, &mut limited, &[(false, "Until stopped"), (true, "A number of times")]) {
                    config.limit = if limited { ClickLimit::Count { count: 100 } } else { ClickLimit::Forever };
                }
                if let ClickLimit::Count { count } = &mut config.limit {
                    ui.add(DragValue::new(count).range(1..=1_000_000_000).suffix(" ×"));
                }
            });
        });

        card(ui, "Where", |ui| {
            let mut fixed = matches!(config.position, ClickPosition::Fixed { .. });
            if widgets::segmented(ui, &mut fixed, &[(false, "At the pointer"), (true, "Fixed position")]) {
                config.position = if fixed { ClickPosition::Fixed { x: 0, y: 0 } } else { ClickPosition::Cursor };
            }
            if let ClickPosition::Fixed { x, y } = &mut config.position {
                ui.horizontal(|ui| {
                    ui.add(DragValue::new(x).prefix("X "));
                    ui.add(DragValue::new(y).prefix("Y "));
                    if ui
                        .button(format!("{}  Pick", icon::CROSSHAIR))
                        .on_hover_text("Choose the position on screen")
                        .clicked()
                    {
                        events
                            .push(ClickerEvent::Request(UiRequest::Snip(SnipMode::Point, SnipTarget::ClickerPosition)));
                    }
                });
            }
        });

        card(ui, "Hotkey", |ui| {
            ui.horizontal(|ui| {
                let pending =
                    env.capturing.filter(|(target, _)| **target == HotkeyTarget::Clicker).map(|(_, pending)| pending);
                match widgets::hotkey_button(ui, &config.hotkey, pending) {
                    HotkeyAction::Capture => {
                        events.push(ClickerEvent::Request(UiRequest::CaptureHotkey(HotkeyTarget::Clicker)))
                    }
                    HotkeyAction::Clear => config.hotkey = Hotkey::none(),
                    HotkeyAction::None => {}
                }
                widgets::segmented(
                    &mut *ui,
                    &mut config.activation,
                    &[(StartMode::Toggle, "Toggle"), (StartMode::Hold, "Hold")],
                );
            });
            let explanation = match config.activation {
                StartMode::Hold => "Clicks for as long as the hotkey is held down.",
                _ => "Press once to start, again to stop.",
            };
            ui.label(RichText::new(explanation).size(11.5).color(theme::TEXT_FAINT));
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            self.presets_menu(ui, config, env, events);
            if ui
                .button(format!("{}  Open as macro", icon::TREE_STRUCTURE))
                .on_hover_text("Turn these settings into blocks you can extend in the Macro tab")
                .clicked()
            {
                events.push(ClickerEvent::OpenAsMacro);
            }
        });
        ui.add_space(8.0);
    }

    fn presets_menu(
        &mut self,
        ui: &mut Ui,
        config: &mut AutoClickerConfig,
        env: &mut ClickerEnv<'_>,
        events: &mut Vec<ClickerEvent>,
    ) {
        let mut delete = None;
        ui.menu_button(format!("{}  Presets", icon::BOOKMARK_SIMPLE), |ui| {
            ui.set_min_width(210.0);
            let mut any = false;
            for entry in env.presets.entries() {
                let PresetBody::AutoClicker(saved) = &entry.preset.body else { continue };
                any = true;
                ui.horizontal(|ui| {
                    if ui.button(&entry.preset.name).clicked() {
                        *config = saved.clone();
                        ui.close();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon_button(ui, icon::TRASH, "Delete this preset").clicked() {
                            delete = Some(entry.path.clone());
                        }
                    });
                });
            }
            if !any {
                ui.weak("No presets saved yet.");
            }
            ui.separator();
            if ui.button(format!("{}  Save current settings…", icon::FLOPPY_DISK)).clicked() {
                self.new_preset = Some(String::new());
                ui.close();
            }
            if ui.button(format!("{}  Import a .mp file…", icon::DOWNLOAD_SIMPLE)).clicked() {
                if let Some(file) = rfd::FileDialog::new().add_filter("MacroMagi preset", &["mp"]).pick_file() {
                    match env.presets.import(&file) {
                        Ok(preset) => events.push(ClickerEvent::Toast(format!("Imported “{}”", preset.name))),
                        Err(error) => events.push(ClickerEvent::Toast(format!("Could not import: {error}"))),
                    }
                }
                ui.close();
            }
        });
        if let Some(path) = delete {
            env.presets.delete(&path);
        }

        if let Some(name) = &mut self.new_preset {
            let mut done = false;
            let field = ui.add(egui::TextEdit::singleline(name).hint_text("Preset name").desired_width(120.0));
            let confirmed = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Save").clicked() || confirmed) && !name.trim().is_empty() {
                let preset = Preset {
                    name: name.trim().to_owned(),
                    description: String::new(),
                    body: PresetBody::AutoClicker(config.clone()),
                };
                match env.presets.save(&preset) {
                    Ok(_) => events.push(ClickerEvent::Toast(format!("Saved preset “{}”", preset.name))),
                    Err(error) => events.push(ClickerEvent::Toast(format!("Could not save the preset: {error}"))),
                }
                done = true;
            }
            if widgets::icon_button(ui, icon::X, "Cancel").clicked() {
                done = true;
            }
            if done {
                self.new_preset = None;
            }
        }
    }

    fn footer(&self, ui: &mut Ui, config: &AutoClickerConfig, env: &ClickerEnv<'_>, events: &mut Vec<ClickerEvent>) {
        if let Some(error) = &env.error {
            ui.label(RichText::new(format!("{}  {error}", icon::WARNING)).size(12.0).color(theme::DANGER));
        }
        let hotkey = if config.hotkey.is_none() { String::new() } else { format!("   ({})", config.hotkey.label()) };
        let (label, color) = if env.running {
            (format!("{}  Stop{hotkey}", icon::STOP), theme::DANGER)
        } else {
            (format!("{}  Start{hotkey}", icon::PLAY), theme::ACCENT)
        };
        let size = vec2(ui.available_width(), 42.0);
        let button = widgets::accent_button(ui, label, color, size);
        if button.clicked() {
            events.push(ClickerEvent::Toggle);
        }
        let status = match (env.running, env.clicks) {
            (true, n) => format!("Clicking…  {n} so far"),
            (false, 0) => "Ready".to_owned(),
            (false, n) => format!("Stopped after {n} clicks"),
        };
        ui.vertical_centered(|ui| {
            let color = if env.running { theme::SUCCESS } else { theme::TEXT_FAINT };
            ui.label(RichText::new(status).size(11.5).color(color));
        });
        let _ = Color32::WHITE;
    }
}
