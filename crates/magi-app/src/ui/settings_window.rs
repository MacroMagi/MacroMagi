//! The settings window: movable and resizable, one page per topic.

use egui::{Color32, RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use magi_core::graph::NodeId;
use magi_core::hotkey::Hotkey;
use magi_core::nodes::flow::StartMode;
use magi_core::project::EnginePreference;
use magi_core::recording::MouseRecording;
use magi_engine::PythonStatus;

use super::widgets::{self, HotkeyAction};
use super::{HotkeyTarget, UiRequest};
use crate::components::{Component, Status as ComponentStatus};
use crate::settings::{Settings, SettingsPage, UI_SCALE_RANGE};
use crate::theme::{self, ThemeMode};

/// Theme colors offered as swatches; any other can be picked.
const SWATCHES: [[u8; 3]; 8] = [
    [0x9B, 0x6D, 0xFF],
    [0x4D, 0xA3, 0xFF],
    [0x2D, 0xD4, 0xC0],
    [0x5B, 0xD1, 0x6B],
    [0xFF, 0xB5, 0x4A],
    [0xFF, 0x7A, 0x45],
    [0xF0, 0x5D, 0x6A],
    [0xFF, 0x7A, 0xB8],
];
/// The interface size is applied this long after the slider stops moving.
const SCALE_DELAY: f64 = 0.35;

/// A Start block of the open macro, for the hotkeys page.
pub struct StartHotkey {
    pub node: NodeId,
    pub name: String,
    pub hotkey: Hotkey,
}

pub struct SettingsEnv<'a> {
    pub capturing: Option<(&'a HotkeyTarget, &'a Hotkey)>,
    pub python: &'a PythonStatus,
    pub starts: &'a [StartHotkey],
    /// Pictures already in the backgrounds folder.
    pub backgrounds: &'a [String],
    /// A background is being downloaded.
    pub downloading: bool,
    /// Python interpreters found by the last check; `None` before the first.
    pub pythons: Option<&'a [(std::path::PathBuf, String)]>,
    /// The interpreter "automatic" stood for at the last check.
    pub python_auto: Option<&'a (std::path::PathBuf, String)>,
    /// A check for Python is under way.
    pub looking_for_python: bool,
    /// The Python downloaded from this page, if there is one.
    pub python_downloaded: Option<std::path::PathBuf>,
    pub gpus: &'a [magi_platform::gpu::Gpu],
    /// The optional downloads and where each stands (empty until worked out).
    pub components: &'a [(Component, ComponentStatus)],
    /// The part being installed or removed, what it is doing, and the share done when known.
    pub installing: Option<(Component, String, Option<f32>)>,
}

pub enum SettingsEvent {
    Request(UiRequest),
    /// Look for Pythons, and try to start the one macros use.
    CheckPython,
    ChooseBackgroundFile,
    DownloadBackground(String),
    RemoveBackground(String),
    ResetHotkeys,
    ResetAll,
    /// A Start block's hotkey was changed or cleared here.
    SetStartHotkey(NodeId, Hotkey),
    /// Work out the optional downloads' statuses.
    CheckComponents,
    Install(Component),
    Remove(Component),
}

#[derive(Default)]
pub struct SettingsWindow {
    pub open: bool,
    url: String,
    confirm_reset: bool,
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title.to_uppercase()).size(10.5).color(theme::text_faint()));
    ui.add_space(2.0);
}

fn hotkey_row(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    hotkey: &mut Hotkey,
    target: HotkeyTarget,
    env: &SettingsEnv<'_>,
    events: &mut Vec<SettingsEvent>,
) -> Option<Hotkey> {
    let mut set = None;
    ui.horizontal(|ui| {
        widgets::fixed_label(ui, label, 150.0).on_hover_text(hint);
        let pending = env.capturing.filter(|(active, _)| **active == target).map(|(_, pending)| pending);
        match widgets::hotkey_button(ui, hotkey, pending) {
            HotkeyAction::Capture => events.push(SettingsEvent::Request(UiRequest::CaptureHotkey(target))),
            HotkeyAction::Cancel => events.push(SettingsEvent::Request(UiRequest::CancelCapture)),
            HotkeyAction::Clear => {
                *hotkey = Hotkey::none();
                set = Some(Hotkey::none());
            }
            HotkeyAction::Set(new) => {
                *hotkey = new.clone();
                set = Some(new);
            }
            HotkeyAction::None => {}
        }
    });
    set
}

impl SettingsWindow {
    pub fn show(&mut self, ctx: &egui::Context, settings: &mut Settings, env: &SettingsEnv<'_>) -> Vec<SettingsEvent> {
        let mut events = Vec::new();
        if !self.open {
            return events;
        }
        let mut open = true;
        let screen = ctx.content_rect();
        let default = settings.settings_size.map_or(vec2(480.0, 540.0), |[w, h]| vec2(w, h));
        let response = egui::Window::new(format!("{}  Settings", icon::GEAR))
            .id(egui::Id::new("settings-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .movable(true)
            .constrain(true)
            .default_size(default.min(screen.size() - vec2(20.0, 20.0)))
            .min_size(vec2(300.0, 240.0))
            .default_pos(screen.center() - default / 2.0)
            .show(ctx, |ui| self.contents(ui, settings, env, &mut events));
        if let Some(response) = response {
            let size = response.response.rect.size();
            settings.settings_size = Some([size.x.round(), size.y.round()]);
        }
        self.open = open;
        events
    }

    fn contents(
        &mut self,
        ui: &mut Ui,
        settings: &mut Settings,
        env: &SettingsEnv<'_>,
        events: &mut Vec<SettingsEvent>,
    ) {
        let narrow = ui.available_width() < 430.0;
        let mut page_ui = |ui: &mut Ui, settings: &mut Settings| {
            // Scrolls both ways: a narrow window shows less, it does not grow.
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                match settings.settings_page {
                    SettingsPage::General => self.general(ui, settings, events),
                    SettingsPage::Appearance => self.appearance(ui, settings, env, events),
                    SettingsPage::Hotkeys => hotkeys(ui, settings, env, events),
                    SettingsPage::Recording => recording(ui, settings),
                    SettingsPage::Engine => engine(ui, settings, env, events),
                    SettingsPage::Ai => ai(ui, settings, env, events),
                }
                ui.add_space(8.0);
            });
        };
        if narrow {
            egui::ComboBox::from_id_salt("settings-page")
                .width(ui.available_width())
                .selected_text(settings.settings_page.label())
                .show_ui(ui, |ui| {
                    for page in SettingsPage::ALL {
                        ui.selectable_value(&mut settings.settings_page, page, page.label());
                    }
                });
            ui.separator();
            page_ui(ui, settings);
        } else {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(118.0);
                    for page in SettingsPage::ALL {
                        let glyph = match page {
                            SettingsPage::General => icon::SLIDERS_HORIZONTAL,
                            SettingsPage::Appearance => icon::PALETTE,
                            SettingsPage::Hotkeys => icon::KEYBOARD,
                            SettingsPage::Recording => icon::RECORD,
                            SettingsPage::Engine => icon::LIGHTNING,
                            SettingsPage::Ai => icon::BRAIN,
                        };
                        let selected = settings.settings_page == page;
                        let text = RichText::new(format!("{glyph}  {}", page.label())).color(if selected {
                            theme::text()
                        } else {
                            theme::text_dim()
                        });
                        let button =
                            egui::Button::new(text).selected(selected).frame(selected).min_size(vec2(118.0, 26.0));
                        if ui.add(button).clicked() {
                            settings.settings_page = page;
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| page_ui(ui, settings));
            });
        }
    }

    fn general(&mut self, ui: &mut Ui, settings: &mut Settings, events: &mut Vec<SettingsEvent>) {
        section(ui, "Window");
        widgets::toggle_row(ui, "Keep MacroMagi on top of other windows", "", &mut settings.always_on_top);
        widgets::toggle_row(
            ui,
            "Hide MacroMagi from screen captures",
            "Keeps this window out of what macros see, and out of images you capture for them. It also hides the \
             window from screenshots and screen sharing – turn it off to record a tutorial.",
            &mut settings.hide_from_capture,
        );
        ui_scale(ui, settings);

        section(ui, "Exporting");
        widgets::note(ui, "What the Export dialog includes unless you change it there.");
        widgets::toggle_row(
            ui,
            "My presets",
            "Bundle your preset library into the file, so the presets travel with it.",
            &mut settings.export.presets,
        );
        widgets::toggle_row(
            ui,
            "Options",
            "Stop hotkeys, program rules and the other Options, so they apply wherever the file is opened.",
            &mut settings.export.options,
        );
        widgets::toggle_row(ui, "Auto Clicker settings", "", &mut settings.export.clicker);

        section(ui, "Safety");
        widgets::toggle_row(
            ui,
            "Dry run: do not send any input",
            "Macros run normally but nothing is pressed or clicked. The console lists what would have been sent.",
            &mut settings.dry_run,
        );

        section(ui, "Reset");
        if self.confirm_reset {
            widgets::notice(
                ui,
                icon::WARNING,
                "Every setting goes back to its default, hotkeys included.",
                theme::warning(),
            );
            ui.horizontal(|ui| {
                if widgets::accent_button(ui, "Reset everything", theme::danger(), vec2(130.0, 26.0)).clicked() {
                    events.push(SettingsEvent::ResetAll);
                    self.confirm_reset = false;
                }
                if ui.button("Cancel").clicked() {
                    self.confirm_reset = false;
                }
            });
        } else if ui.button(format!("{}  Reset all settings…", icon::ARROW_COUNTER_CLOCKWISE)).clicked() {
            self.confirm_reset = true;
        }

        section(ui, "About");
        about(ui);
    }

    fn appearance(
        &mut self,
        ui: &mut Ui,
        settings: &mut Settings,
        env: &SettingsEnv<'_>,
        events: &mut Vec<SettingsEvent>,
    ) {
        let look = &mut settings.appearance;
        section(ui, "Theme");
        widgets::segmented(
            ui,
            &mut look.theme,
            &[(ThemeMode::System, ThemeMode::System.label()), (ThemeMode::Light, "Light"), (ThemeMode::Dark, "Dark")],
        );

        section(ui, "Theme color");
        widgets::note(ui, "Colors every part of MacroMagi, icons and blocks included.");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            // The rings reach past the circles; keep them clear of the edge.
            ui.add_space(4.0);
            let mono = look.accent.is_none();
            if swatch(ui, None, mono).on_hover_text("Monochrome").clicked() {
                look.accent = None;
            }
            for rgb in SWATCHES {
                if swatch(ui, Some(rgb), look.accent == Some(rgb)).clicked() {
                    look.accent = Some(rgb);
                }
            }
            let mut custom = look.accent.unwrap_or([0x9B, 0x6D, 0xFF]);
            if ui.color_edit_button_srgb(&mut custom).on_hover_text("Any color").changed() {
                look.accent = Some(custom);
            }
        });

        section(ui, "Background");
        ui.horizontal_wrapped(|ui| {
            let current = look.background.clone().unwrap_or_else(|| "None".to_owned());
            egui::ComboBox::from_id_salt("background").width(170.0).selected_text(current).show_ui(ui, |ui| {
                if ui.selectable_label(look.background.is_none(), "None").clicked() {
                    look.background = None;
                }
                for name in env.backgrounds {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(look.background.as_ref() == Some(name), name).clicked() {
                            look.background = Some(name.clone());
                        }
                        if widgets::icon_button(ui, icon::TRASH, "Delete this picture").clicked() {
                            events.push(SettingsEvent::RemoveBackground(name.clone()));
                        }
                    });
                }
            });
            if ui.button(format!("{}  File…", icon::FOLDER_OPEN)).on_hover_text("PNG, JPEG, GIF, WebP or BMP").clicked()
            {
                events.push(SettingsEvent::ChooseBackgroundFile);
            }
        });
        ui.horizontal(|ui| {
            let field = ui.add_enabled(
                !env.downloading,
                egui::TextEdit::singleline(&mut self.url)
                    .hint_text("https://… picture or GIF address")
                    .desired_width(ui.available_width() - 90.0),
            );
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let label =
                if env.downloading { "Loading…".to_owned() } else { format!("{}  Add", icon::DOWNLOAD_SIMPLE) };
            let ready = !env.downloading && !self.url.trim().is_empty();
            if (ui.add_enabled(ready, egui::Button::new(label)).clicked() || (entered && ready)) && ready {
                events.push(SettingsEvent::DownloadBackground(self.url.trim().to_owned()));
                self.url.clear();
            }
        });
        ui.add_enabled_ui(look.background.is_some(), |ui| {
            percent_slider(ui, "Picture", "How strongly the picture shows.", &mut look.background_opacity);
            ui.horizontal(|ui| {
                widgets::fixed_label(ui, RichText::new("Blur").color(theme::text_dim()), 110.0);
                ui.add(egui::Slider::new(&mut look.background_blur, 0.0..=40.0).suffix(" px").fixed_decimals(0));
            });
            percent_slider(
                ui,
                "Darkness",
                "Darkens the picture (lightens it in light mode) so text stays readable.",
                &mut look.background_darkness,
            );
            percent_slider(ui, "Panel opacity", "How solid the panels over the picture are.", &mut look.panel_opacity);
        });
    }
}

fn percent_slider(ui: &mut Ui, label: &str, hint: &str, value: &mut f32) {
    ui.horizontal(|ui| {
        widgets::fixed_label(ui, RichText::new(label).color(theme::text_dim()), 110.0).on_hover_text(hint);
        let mut percent = *value * 100.0;
        if ui.add(egui::Slider::new(&mut percent, 0.0..=100.0).suffix("%").fixed_decimals(0)).changed() {
            *value = percent / 100.0;
        }
    });
}

/// A round color swatch; `None` is the monochrome swatch (half black, half white).
fn swatch(ui: &mut Ui, rgb: Option<[u8; 3]>, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
    let painter = ui.painter();
    let center = rect.center();
    match rgb {
        Some([r, g, b]) => {
            painter.circle_filled(center, 9.0, Color32::from_rgb(r, g, b));
        }
        None => {
            painter.circle_filled(center, 9.0, Color32::BLACK);
            let half: Vec<egui::Pos2> = (0..=16)
                .map(|i| {
                    let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 16.0;
                    center + 9.0 * vec2(angle.cos(), angle.sin())
                })
                .collect();
            painter.add(egui::Shape::convex_polygon(half, Color32::WHITE, egui::Stroke::NONE));
        }
    }
    let ring = if selected {
        theme::text()
    } else if response.hovered() {
        theme::text_dim()
    } else {
        theme::stroke_strong()
    };
    painter.circle_stroke(center, 10.5, egui::Stroke::new(if selected { 2.0 } else { 1.0 }, ring));
    response
}

/// The interface size: a slider that applies a moment after it stops moving,
/// and a value that can be typed as a percentage.
fn ui_scale(ui: &mut Ui, settings: &mut Settings) {
    let id = egui::Id::new("ui-scale-pending");
    let now = ui.input(|i| i.time);
    let (mut pending, mut changed_at): (f32, f64) =
        ui.data(|d| d.get_temp(id)).unwrap_or((settings.ui_scale, f64::NEG_INFINITY));
    ui.horizontal(|ui| {
        widgets::fixed_label(ui, RichText::new("Interface size").color(theme::text_dim()), 110.0);
        let slider = egui::Slider::new(&mut pending, UI_SCALE_RANGE)
            .step_by(0.05)
            .update_while_editing(false)
            .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
            .custom_parser(|text| {
                let number: f64 = text.trim().trim_end_matches('%').trim().replace(',', ".").parse().ok()?;
                // "125" and "125%" mean 125%; "1.25" means the same.
                Some(if number > 5.0 { number / 100.0 } else { number })
            });
        let response = ui.add(slider);
        if response.changed() {
            changed_at = now;
        }
        if widgets::reset_button(ui, (pending - 1.0).abs() > 1e-4) {
            pending = 1.0;
            changed_at = f64::NEG_INFINITY;
        }
        let dragging = response.dragged() || ui.input(|i| i.pointer.any_down());
        if (pending - settings.ui_scale).abs() > 1e-4 {
            if !dragging && now - changed_at >= SCALE_DELAY {
                settings.ui_scale = pending;
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
            }
        } else {
            // Follows changes made elsewhere (reset, another window).
            pending = settings.ui_scale;
        }
    });
    ui.data_mut(|d| d.insert_temp(id, (pending, changed_at)));
}

fn hotkeys(ui: &mut Ui, settings: &mut Settings, env: &SettingsEnv<'_>, events: &mut Vec<SettingsEvent>) {
    widgets::note(ui, "Any key, key combination, mouse button or gamepad button. Hotkeys work in every program.");

    section(ui, "Auto Clicker");
    hotkey_row(
        ui,
        "Start / stop",
        "Starts and stops the auto clicker while the Auto Clicker tab is open.",
        &mut settings.clicker.hotkey,
        HotkeyTarget::Clicker,
        env,
        events,
    );
    ui.horizontal(|ui| {
        ui.add_space(154.0);
        widgets::segmented(
            ui,
            &mut settings.clicker.activation,
            &[(StartMode::Toggle, "Toggle"), (StartMode::Hold, "Hold")],
        );
    });

    section(ui, "Macro");
    hotkey_row(
        ui,
        "Run / stop",
        "Like the Run button: starts the macro, or stops it.",
        &mut settings.run_hotkey,
        HotkeyTarget::Run,
        env,
        events,
    );
    hotkey_row(
        ui,
        "Record",
        "Starts and stops recording your input.",
        &mut settings.record_hotkey,
        HotkeyTarget::Record,
        env,
        events,
    );
    for start in env.starts {
        let mut hotkey = start.hotkey.clone();
        if let Some(new) = hotkey_row(
            ui,
            &start.name,
            "A Start block of the open macro",
            &mut hotkey,
            HotkeyTarget::Node(start.node, "hotkey".into()),
            env,
            events,
        ) {
            events.push(SettingsEvent::SetStartHotkey(start.node, new));
        }
    }

    section(ui, "Stopping");
    hotkey_row(
        ui,
        "Stop everything",
        "Stops the auto clicker and every running macro, whatever has focus.",
        &mut settings.stop_hotkey,
        HotkeyTarget::StopEverything,
        env,
        events,
    );
    let mut remove = None;
    for (index, hotkey) in settings.options.stop_hotkeys.iter_mut().enumerate() {
        ui.push_id(("extra-stop", index), |ui| {
            if let Some(new) = hotkey_row(
                ui,
                "Also stops",
                "An extra stop hotkey",
                hotkey,
                HotkeyTarget::ExtraStop(Some(index)),
                env,
                events,
            ) && new.is_none()
            {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        settings.options.stop_hotkeys.remove(index);
    }
    let adding = HotkeyTarget::ExtraStop(None);
    if env.capturing.is_some_and(|(t, _)| *t == adding) {
        let mut empty = Hotkey::none();
        hotkey_row(ui, "Also stops", "", &mut empty, adding, env, events);
    } else if ui.button(format!("{}  Add another stop hotkey", icon::PLUS)).clicked() {
        events.push(SettingsEvent::Request(UiRequest::CaptureHotkey(adding)));
    }
    widgets::toggle_row(ui, "Stop on task switch (Alt+Tab, Win+Tab)", "", &mut settings.options.stop_on_task_switch);
    widgets::toggle_row(
        ui,
        "Strict hotkey modifiers",
        "On: a hotkey works only when exactly its modifier keys are held. Off: extra held modifiers are ignored.",
        &mut settings.options.strict_modifiers,
    );

    let conflicts = conflicts(settings, env.starts);
    if !conflicts.is_empty() {
        ui.add_space(6.0);
        for hotkey in conflicts {
            widgets::notice(ui, icon::WARNING, format!("{} is used more than once.", hotkey.label()), theme::warning());
        }
    }

    ui.add_space(10.0);
    if ui
        .button(format!("{}  Reset all hotkeys", icon::ARROW_COUNTER_CLOCKWISE))
        .on_hover_text("Puts every hotkey above back to its default. Start blocks keep theirs.")
        .clicked()
    {
        events.push(SettingsEvent::ResetHotkeys);
    }
}

/// Hotkeys bound to more than one thing that can be active at the same time.
fn conflicts(settings: &Settings, starts: &[StartHotkey]) -> Vec<Hotkey> {
    let global = [&settings.stop_hotkey, &settings.record_hotkey];
    let check = |tab_specific: Vec<&Hotkey>| {
        let mut all: Vec<&Hotkey> =
            global.iter().copied().chain(settings.options.stop_hotkeys.iter()).chain(tab_specific).collect();
        all.retain(|h| !h.is_none());
        let mut seen: Vec<&Hotkey> = Vec::new();
        let mut doubled = Vec::new();
        for hotkey in all {
            if seen.contains(&hotkey) {
                if !doubled.contains(hotkey) {
                    doubled.push(hotkey.clone());
                }
            } else {
                seen.push(hotkey);
            }
        }
        doubled
    };
    let mut found = check(vec![&settings.clicker.hotkey]);
    let macro_keys: Vec<&Hotkey> =
        std::iter::once(&settings.run_hotkey).chain(starts.iter().map(|s| &s.hotkey)).collect();
    for hotkey in check(macro_keys) {
        if !found.contains(&hotkey) {
            found.push(hotkey);
        }
    }
    found
}

fn recording(ui: &mut Ui, settings: &mut Settings) {
    section(ui, "Record");
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut settings.record.keyboard, "Keyboard");
        ui.checkbox(&mut settings.record.mouse_buttons, "Mouse buttons");
        ui.checkbox(&mut settings.record.mouse_moves, "Mouse movement");
        ui.checkbox(&mut settings.record.scroll, "Scroll wheel");
        ui.checkbox(&mut settings.record.gamepad, "Gamepad");
    });

    section(ui, "Mouse movement");
    for mode in MouseRecording::ALL {
        ui.radio_value(&mut settings.record.mouse_mode, mode, mode.label());
    }
    widgets::note(
        ui,
        "Games that lock the pointer (first-person cameras) keep moving it back to the middle. Smart recording \
         notices this and records how far the mouse moved instead, so the camera turns when the recording plays.",
    );

    section(ui, "After recording");
    widgets::toggle_row(ui, "Skip the pause before the first input", "", &mut settings.record.trim_start);
    widgets::toggle_row(
        ui,
        "Add a Play Recording block when a recording ends",
        "",
        &mut settings.auto_insert_recording,
    );
    ui.add_space(4.0);
    widgets::notice(
        ui,
        icon::WARNING,
        "A recording contains everything you type while it runs, passwords included.",
        theme::warning(),
    );
}

fn engine(ui: &mut Ui, settings: &mut Settings, env: &SettingsEnv<'_>, events: &mut Vec<SettingsEvent>) {
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
    widgets::note(
        ui,
        match settings.engine {
            EnginePreference::Auto => "Uses Python when it is installed, otherwise the built-in native engine.",
            EnginePreference::Python => "Always uses Python. Needed for Python Script blocks.",
            EnginePreference::Native => "Always uses the built-in Rust engine. Needs nothing installed.",
        },
    );

    section(ui, "Python");
    ui.horizontal_wrapped(|ui| {
        let (text, color) = match env.python {
            PythonStatus::Running { version, command } => (format!("Python {version} ({command})"), theme::success()),
            PythonStatus::Unknown => ("Python has not been started yet".to_owned(), theme::text_dim()),
            PythonStatus::Unavailable(reason) => (reason.clone(), theme::warning()),
        };
        let busy = env.looking_for_python;
        let check = egui::Button::new(if busy {
            "Checking…".to_owned()
        } else {
            format!("{}  Check", icon::ARROWS_CLOCKWISE)
        });
        if ui
            .add_enabled(!busy, check)
            .on_hover_text("Look for Pythons on this computer, and start the one macros use to see whether it works")
            .clicked()
        {
            events.push(SettingsEvent::CheckPython);
        }
        ui.add(egui::Label::new(RichText::new(text).size(11.5).color(color)).wrap());
    });
    // Which Python: whatever "automatic" finds, one of those found, or any file.
    let found = env.pythons.unwrap_or_default();
    let describe = |path: &std::path::Path| match found.iter().find(|(p, _)| p == path) {
        Some((_, version)) => format!("Python {version} – {}", path.display()),
        None => path.display().to_string(),
    };
    let auto = match (env.python_auto, env.pythons) {
        (Some((path, version)), _) => format!("(auto) Python {version} – {}", path.display()),
        (None, Some(_)) => "(auto) no Python found".to_owned(),
        (None, None) => "(auto) not checked yet – press Check".to_owned(),
    };
    let shown = settings.python_path.as_deref().map_or_else(|| auto.clone(), describe);
    egui::ComboBox::from_id_salt("python-choice")
        .width(ui.available_width() - 8.0)
        .truncate()
        .selected_text(shown)
        .show_ui(ui, |ui| {
            if ui.selectable_label(settings.python_path.is_none(), &auto).clicked() {
                settings.python_path = None;
            }
            for (path, version) in found {
                let chosen = settings.python_path.as_deref() == Some(path.as_path());
                if ui.selectable_label(chosen, format!("Python {version} – {}", path.display())).clicked() {
                    settings.python_path = Some(path.clone());
                }
            }
            if ui.button(format!("{}  Another file…", icon::FOLDER_OPEN)).clicked()
                && let Some(file) = rfd::FileDialog::new().set_title("Choose the Python interpreter").pick_file()
            {
                settings.python_path = Some(file);
            }
        });
    match env.pythons {
        None => widgets::note(ui, "Press Check to look for Python. It is also looked for the first time a macro runs."),
        Some([]) => widgets::notice(ui, icon::WARNING, "No Python was found on this computer.", theme::warning()),
        Some(found) => widgets::note(
            ui,
            if found.len() == 1 {
                "1 Python installation found.".to_owned()
            } else {
                format!("{} Python installations found.", found.len())
            },
        ),
    }

    section(ui, "Download Python");
    match Component::Python.unavailable() {
        Some(_) => {
            widgets::note(
                ui,
                "Python comes with most Linux and macOS systems; install it from python.org if it is missing.",
            );
            if ui.button(format!("{}  Get Python from python.org", icon::ARROW_SQUARE_OUT)).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab("https://www.python.org/downloads/"));
            }
        }
        None => {
            // Without the full check (which the AI page runs), whether the folder is there is enough.
            let known = env.components.iter().find(|(c, _)| *c == Component::Python).map(|(_, s)| s.clone());
            let status = known.unwrap_or_else(|| match &env.python_downloaded {
                Some(path) => ComponentStatus::Installed { bytes: 0, detail: path.display().to_string() },
                None => ComponentStatus::Missing,
            });
            component_card(ui, Component::Python, Some(&status), env, events);
        }
    }
    widgets::note(
        ui,
        "An unused Python process is stopped after a few minutes to save memory; it starts again on the next run.",
    );
}

/// One optional download: what it is, where it stands, and Install or Uninstall.
fn component_card(
    ui: &mut Ui,
    component: Component,
    status: Option<&ComponentStatus>,
    env: &SettingsEnv<'_>,
    events: &mut Vec<SettingsEvent>,
) {
    egui::Frame::new().fill(theme::bg_raised()).corner_radius(theme::RADIUS).inner_margin(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(component.name()).strong());
            let (text, color) = match status {
                Some(ComponentStatus::Installed { bytes, detail }) if *bytes > 0 => {
                    (format!("Installed · {detail} · {} MB", bytes / (1024 * 1024)), theme::success())
                }
                Some(ComponentStatus::Installed { detail, .. }) => (format!("Installed · {detail}"), theme::success()),
                Some(ComponentStatus::OnSystem(detail)) => (format!("Found · {detail}"), theme::success()),
                Some(ComponentStatus::Missing) => ("Not installed".to_owned(), theme::text_faint()),
                Some(ComponentStatus::RemovedAfterRestart) => {
                    ("Uninstalled when MacroMagi restarts".to_owned(), theme::warning())
                }
                None => ("…".to_owned(), theme::text_faint()),
            };
            ui.add(egui::Label::new(RichText::new(text).size(11.5).color(color)).truncate());
        });
        widgets::note(ui, component.description());
        match (&env.installing, component.unavailable()) {
            (Some((busy, message, share)), _) if *busy == component => {
                widgets::progress_bar(ui, *share);
                ui.label(RichText::new(message).size(11.0).color(theme::text_dim()));
            }
            (_, Some(reason)) => widgets::note(ui, reason),
            (installing, None) => {
                let idle = installing.is_none();
                ui.horizontal_wrapped(|ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    let installed = matches!(status, Some(ComponentStatus::Installed { .. }));
                    if !installed {
                        let label = format!("{}  Install ({})", icon::DOWNLOAD_SIMPLE, component.size());
                        if ui.add_enabled(idle, egui::Button::new(label)).clicked() {
                            events.push(SettingsEvent::Install(component));
                        }
                    }
                    if installed
                        && ui.add_enabled(idle, egui::Button::new(format!("{}  Uninstall", icon::TRASH))).clicked()
                    {
                        events.push(SettingsEvent::Remove(component));
                    }
                });
            }
        }
    });
}

fn ai(ui: &mut Ui, settings: &mut Settings, env: &SettingsEnv<'_>, events: &mut Vec<SettingsEvent>) {
    if env.components.is_empty() {
        events.push(SettingsEvent::CheckComponents);
    }
    section(ui, "Where AI models run");
    let runtime = magi_ml::onnxruntime();
    let gpu_ready = runtime.as_ref().is_some_and(|(_, gpu)| *gpu);
    let label = |device: magi_ml::Device| match device {
        magi_ml::Device::Auto => "Automatic – whatever is fastest".to_owned(),
        magi_ml::Device::Cpu => "The CPU".to_owned(),
        magi_ml::Device::Gpu { index } => env
            .gpus
            .iter()
            .find(|g| g.index == index)
            .map_or_else(|| format!("Graphics card {}", index + 1), |g| format!("{} ({} MB)", g.name, g.memory_mb)),
    };
    egui::ComboBox::from_id_salt("ai-device")
        .width(ui.available_width() - 8.0)
        .truncate()
        .selected_text(label(settings.ai_device))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut settings.ai_device, magi_ml::Device::Auto, label(magi_ml::Device::Auto));
            ui.selectable_value(&mut settings.ai_device, magi_ml::Device::Cpu, label(magi_ml::Device::Cpu));
            for gpu in env.gpus {
                let device = magi_ml::Device::Gpu { index: gpu.index };
                ui.add_enabled_ui(gpu_ready, |ui| {
                    ui.selectable_value(&mut settings.ai_device, device, label(device))
                        .on_disabled_hover_text("Needs ONNX Runtime for graphics cards (below)");
                });
            }
        });
    widgets::note(
        ui,
        "Automatic measures each way on every model and keeps the fastest – for tiny models that is often the \
         CPU. Image Detection always runs on the CPU, where its coarse-to-fine search is fastest.",
    );
    widgets::note(
        ui,
        match &runtime {
            Some((path, true)) => format!("Using ONNX Runtime with graphics card support ({}).", path.display()),
            Some((path, false)) => format!("Using ONNX Runtime on the CPU ({}).", path.display()),
            None => "Using the built-in engine (CPU). ONNX Runtime makes models faster, and can use a graphics card."
                .to_owned(),
        },
    );

    ui.horizontal(|ui| {
        section(ui, "Downloads");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
            if ui.small_button(format!("{}  Check again", icon::ARROWS_CLOCKWISE)).clicked() {
                events.push(SettingsEvent::CheckComponents);
            }
        });
    });
    widgets::note(
        ui,
        "Parts that some features need. Install them when you want those features; uninstall them any time.",
    );
    for component in Component::AI {
        let status = env.components.iter().find(|(c, _)| *c == component).map(|(_, s)| s);
        component_card(ui, component, status, env, events);
        ui.add_space(4.0);
    }
}

/// Who makes MacroMagi, with links to them.
pub fn about(ui: &mut Ui) {
    ui.label(RichText::new(format!("MacroMagi {}", magi_core::VERSION)).strong());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label("Developed and created by");
        ui.hyperlink_to("MagiGun", "https://github.com/MagiGun");
        ui.label("and");
        ui.hyperlink_to("uvxszi", "https://github.com/uvxszi");
    });
}

#[cfg(test)]
mod tests {
    use magi_core::input::Key;

    use super::*;

    #[test]
    fn conflicts_are_found_only_between_hotkeys_active_together() {
        let mut settings = Settings::default();
        assert!(conflicts(&settings, &[]).is_empty(), "the defaults do not clash");

        // The clicker hotkey and the run hotkey are never active at the same time.
        settings.run_hotkey = settings.clicker.hotkey.clone();
        assert!(conflicts(&settings, &[]).is_empty());

        let start = StartHotkey { node: 1, name: "Start".into(), hotkey: Hotkey::key(Key::F8) };
        assert_eq!(conflicts(&settings, &[start]), [Hotkey::key(Key::F8)], "F8 is also stop everything");
    }
}
