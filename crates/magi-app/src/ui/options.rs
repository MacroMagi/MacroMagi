//! The Options menu and its windows: rules about when MacroMagi may act,
//! shared by the Auto Clicker and macros.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use egui::{RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use magi_core::hotkey::Hotkey;
use magi_core::options::{AutomationOptions, ProcessRule, RuleAction, RuleMode};
use magi_platform::system::ProcessEntry;

use super::widgets::{self, HotkeyAction};
use super::{HotkeyTarget, UiRequest};
use crate::theme;

/// The process list is read at most this often.
const PROCESS_REFRESH: Duration = Duration::from_secs(2);

static PROCESSES: Mutex<Option<(Instant, Vec<ProcessEntry>)>> = Mutex::new(None);

/// Running programs, programs with windows first. Cached briefly: reading the
/// list every frame while a menu is open would be wasteful.
pub fn running_programs(force: bool) -> Vec<ProcessEntry> {
    let mut cache = PROCESSES.lock().unwrap_or_else(|p| p.into_inner());
    let stale = cache.as_ref().is_none_or(|(at, _)| at.elapsed() >= PROCESS_REFRESH);
    if force || stale {
        *cache = Some((Instant::now(), magi_platform::system::running_processes()));
    }
    cache.as_ref().map(|(_, list)| list.clone()).unwrap_or_default()
}

/// Forgets the cached process list, so memory is not held while nobody looks.
pub fn forget_programs() {
    *PROCESSES.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

/// A menu button listing running programs. Returns the one picked.
pub fn program_picker(ui: &mut Ui, id: &str) -> Option<String> {
    let mut picked = None;
    let button = egui::Button::new(icon::LIST_BULLETS).min_size(vec2(26.0, 22.0));
    egui::containers::menu::MenuButton::from_button(button)
        .ui(ui, |ui| {
            ui.set_min_width(220.0);
            let filter_id = ui.id().with((id, "filter"));
            let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
            ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Search programs").desired_width(f32::INFINITY));
            let needle = filter.trim().to_lowercase();
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for program in running_programs(false)
                    .iter()
                    .filter(|p| needle.is_empty() || p.name.to_lowercase().contains(&needle))
                {
                    let glyph = if program.has_window { icon::APP_WINDOW } else { icon::GEAR };
                    if ui.button(format!("{glyph}  {}", program.name)).clicked() {
                        picked = Some(program.name.clone());
                        ui.close();
                    }
                }
            });
            ui.data_mut(|d| d.insert_temp(filter_id, filter));
        })
        .0
        .on_hover_text("Pick from the running programs");
    picked
}

pub enum OptionsEvent {
    OpenStopHotkeys,
    OpenProcessRules,
    CreateShortcut,
}

/// The contents of the Options menu.
pub fn menu(ui: &mut Ui, options: &mut AutomationOptions, dry_run: &mut bool) -> Vec<OptionsEvent> {
    let mut events = Vec::new();
    ui.set_min_width(270.0);
    widgets::toggle_row(
        ui,
        "Strict hotkey modifiers",
        "On: a hotkey works only when exactly its modifier keys are held (Ctrl+F6 never triggers F6).\n\
         Off: extra held modifiers are ignored, so F6 still works while you hold Shift in a game.",
        &mut options.strict_modifiers,
    );
    widgets::toggle_row(
        ui,
        "Stop on task switch",
        "Stop the auto clicker and every macro when you press Alt+Tab or Win+Tab.",
        &mut options.stop_on_task_switch,
    );
    ui.separator();
    let count = options.stop_hotkeys.len();
    let label = if count == 0 {
        format!("{}  Auto-stop hotkeys…", icon::STOP_CIRCLE)
    } else {
        format!("{}  Auto-stop hotkeys ({count})…", icon::STOP_CIRCLE)
    };
    if ui.button(label).on_hover_text("More key combinations that stop everything").clicked() {
        events.push(OptionsEvent::OpenStopHotkeys);
        ui.close();
    }
    let rules = if options.process_rules.enabled { "on" } else { "off" };
    if ui
        .button(format!("{}  Program rules ({rules})…", icon::LIST_CHECKS))
        .on_hover_text("Choose which programs MacroMagi may send input to")
        .clicked()
    {
        events.push(OptionsEvent::OpenProcessRules);
        ui.close();
    }
    ui.menu_button(format!("{}  Behaviours", icon::SLIDERS_HORIZONTAL), |ui| {
        ui.set_min_width(300.0);
        widgets::toggle_row(
            ui,
            "Stop the auto clicker when editing a hotkey",
            "Clicking into a hotkey field stops the auto clicker, so you can press the new hotkey.",
            &mut options.stop_when_editing_hotkey,
        );
    });
    ui.separator();
    widgets::toggle_row(
        ui,
        "Dry run",
        "Macros and the auto clicker run normally but nothing is pressed or clicked. The console lists what would \
         have been sent.",
        dry_run,
    );
    ui.separator();
    if ui.button(format!("{}  Create desktop shortcut", icon::DESKTOP)).clicked() {
        events.push(OptionsEvent::CreateShortcut);
        ui.close();
    }
    events
}

/// The window listing the extra stop hotkeys.
pub fn stop_hotkeys_window(
    ctx: &egui::Context,
    open: &mut bool,
    options: &mut AutomationOptions,
    capturing: Option<(&HotkeyTarget, &Hotkey)>,
) -> Vec<UiRequest> {
    let mut requests = Vec::new();
    egui::Window::new(format!("{}  Auto-stop hotkeys", icon::STOP_CIRCLE))
        .id(egui::Id::new("stop-hotkeys-window"))
        .open(open)
        .collapsible(false)
        .resizable(true)
        .default_width(330.0)
        .show(ctx, |ui| {
            widgets::note(
                ui,
                "Each of these stops the auto clicker and every running macro, wherever you are – in addition to \
                 the stop-everything hotkey.",
            );
            ui.add_space(4.0);
            let mut remove = None;
            for (index, hotkey) in options.stop_hotkeys.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    ui.horizontal(|ui| {
                        let target = HotkeyTarget::ExtraStop(Some(index));
                        let pending = capturing.filter(|(t, _)| **t == target).map(|(_, p)| p);
                        match widgets::hotkey_button(ui, hotkey, pending) {
                            HotkeyAction::Capture => requests.push(UiRequest::CaptureHotkey(target)),
                            HotkeyAction::Cancel => requests.push(UiRequest::CancelCapture),
                            HotkeyAction::Clear => remove = Some(index),
                            HotkeyAction::Set(new) => *hotkey = new,
                            HotkeyAction::None => {}
                        }
                    });
                });
            }
            if let Some(index) = remove {
                options.stop_hotkeys.remove(index);
            }
            let adding = HotkeyTarget::ExtraStop(None);
            match capturing.filter(|(t, _)| **t == adding) {
                Some((_, pending)) => {
                    if let HotkeyAction::Cancel = widgets::hotkey_button(ui, &Hotkey::none(), Some(pending)) {
                        requests.push(UiRequest::CancelCapture);
                    }
                }
                None => {
                    if ui.button(format!("{}  Add a hotkey", icon::PLUS)).clicked() {
                        requests.push(UiRequest::CaptureHotkey(adding));
                    }
                }
            }
        });
    requests
}

/// State of the program rules window between frames.
#[derive(Default)]
pub struct RulesWindow {
    pub open: bool,
    search: String,
    typed_name: String,
}

impl RulesWindow {
    pub fn show(&mut self, ctx: &egui::Context, options: &mut AutomationOptions) {
        if !self.open {
            return;
        }
        let mut open = true;
        egui::Window::new(format!("{}  Program rules", icon::LIST_CHECKS))
            .id(egui::Id::new("process-rules-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([380.0, 460.0])
            .min_width(300.0)
            .show(ctx, |ui| self.contents(ui, options));
        self.open = open;
        if !open {
            forget_programs();
        }
    }

    fn contents(&mut self, ui: &mut Ui, options: &mut AutomationOptions) {
        let rules = &mut options.process_rules;
        widgets::toggle_row(
            ui,
            "Use program rules",
            "When on, MacroMagi only sends input while an allowed program is in front.",
            &mut rules.enabled,
        );
        ui.add_enabled_ui(rules.enabled, |ui| {
            widgets::segmented(
                ui,
                &mut rules.mode,
                &[(RuleMode::Whitelist, "Whitelist"), (RuleMode::Blacklist, "Blacklist")],
            );
            widgets::note(
                ui,
                match rules.mode {
                    RuleMode::Whitelist => {
                        "Input is only sent while one of these programs is in front. The action says what happens \
                         when you switch away from it."
                    }
                    RuleMode::Blacklist => {
                        "Input is never sent while one of these programs is in front. The action says what happens \
                         when it comes to the front."
                    }
                },
            );
            let mode = rules.mode;
            let list = rules.list_mut(mode);

            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "{} ({})",
                    if mode == RuleMode::Whitelist { "ALLOWED" } else { "BLOCKED" },
                    list.len()
                ))
                .size(10.5)
                .color(theme::text_faint()),
            );
            let mut remove = None;
            if list.is_empty() {
                widgets::note(ui, "No programs yet – add them from the list below.");
            }
            for (index, rule) in list.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    ui.horizontal(|ui| {
                        widgets::toggle(ui, &mut rule.enabled).on_hover_text("Switch this rule on or off");
                        ui.add(egui::Label::new(&rule.name).truncate());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::icon_button(ui, icon::TRASH, "Remove").clicked() {
                                remove = Some(index);
                            }
                            egui::ComboBox::from_id_salt("action")
                                .width(70.0)
                                .selected_text(rule.action.label())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut rule.action, RuleAction::Stop, "Stop")
                                        .on_hover_text("End the auto clicker or macro");
                                    ui.selectable_value(&mut rule.action, RuleAction::Pause, "Pause")
                                        .on_hover_text("Wait, and carry on once an allowed program is in front again");
                                });
                        });
                    });
                });
            }
            if let Some(index) = remove {
                list.remove(index);
            }

            ui.separator();
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text(format!("{}  Search running programs", icon::MAGNIFYING_GLASS))
                        .desired_width(ui.available_width() - 30.0),
                );
                if widgets::icon_button(ui, icon::ARROWS_CLOCKWISE, "Refresh the list").clicked() {
                    running_programs(true);
                }
            });
            let needle = self.search.trim().to_lowercase();
            let programs: Vec<ProcessEntry> = running_programs(false)
                .into_iter()
                .filter(|p| needle.is_empty() || p.name.to_lowercase().contains(&needle))
                .filter(|p| !list.iter().any(|rule| rule.name.eq_ignore_ascii_case(&p.name)))
                .collect();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(ui.available_height() - 34.0)
                .show_rows(ui, 24.0, programs.len(), |ui, range| {
                    for program in &programs[range] {
                        ui.horizontal(|ui| {
                            if ui.small_button(icon::PLUS).on_hover_text("Add to the list").clicked() {
                                list.push(ProcessRule {
                                    name: program.name.clone(),
                                    enabled: true,
                                    action: RuleAction::Stop,
                                });
                            }
                            let glyph = if program.has_window { icon::APP_WINDOW } else { icon::GEAR };
                            ui.label(RichText::new(glyph).color(theme::text_faint()));
                            ui.add(egui::Label::new(&program.name).truncate());
                        });
                    }
                });
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.typed_name)
                        .hint_text("Or type a program name, e.g. game.exe")
                        .desired_width(ui.available_width() - 56.0),
                );
                let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let name = self.typed_name.trim().to_owned();
                if (ui.add_enabled(!name.is_empty(), egui::Button::new("Add")).clicked() || entered) && !name.is_empty()
                {
                    if !list.iter().any(|rule| rule.name.eq_ignore_ascii_case(&name)) {
                        list.push(ProcessRule { name, enabled: true, action: RuleAction::Stop });
                    }
                    self.typed_name.clear();
                }
            });
        });
    }
}
