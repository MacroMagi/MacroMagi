//! The Auto Clicker tab: the clicker form, its hotkey, and Start / Stop.

use egui::{RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use magi_core::autoclicker::AutoClickerConfig;
use magi_core::hotkey::Hotkey;
use magi_core::nodes::flow::StartMode;

use super::widgets::{self, HotkeyAction, card};
use super::{HotkeyTarget, SnipTarget, UiRequest, clicker_form};
use crate::theme;

/// Things the tab asks the app to do.
pub enum ClickerEvent {
    Start,
    Stop,
    Request(UiRequest),
}

#[derive(Default)]
pub struct ClickerTab;

pub struct ClickerEnv<'a> {
    pub running: bool,
    /// How many clicks the current (or last) run has made.
    pub clicks: u64,
    pub error: Option<String>,
    pub capturing: Option<(&'a HotkeyTarget, &'a Hotkey)>,
}

impl ClickerTab {
    pub fn show(&mut self, ui: &mut Ui, config: &mut AutoClickerConfig, env: &ClickerEnv<'_>) -> Vec<ClickerEvent> {
        let mut events = Vec::new();

        // Start and Stop stay at the bottom, whatever the window height.
        egui::Panel::bottom("clicker-start")
            .show_separator_line(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 12)))
            .show(ui, |ui| footer(ui, config, env, &mut events));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 14, top: 12, bottom: 0 }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.add_enabled_ui(!env.running, |ui| {
                        for request in clicker_form::show(ui, config, "clicker", SnipTarget::ClickerPosition) {
                            events.push(ClickerEvent::Request(request));
                        }
                        ui.add_space(6.0);
                        hotkey_card(ui, config, env, &mut events);
                        ui.add_space(10.0);
                    });
                });
            });
        events
    }
}

fn hotkey_card(ui: &mut Ui, config: &mut AutoClickerConfig, env: &ClickerEnv<'_>, events: &mut Vec<ClickerEvent>) {
    card(ui, "Hotkey", |ui| {
        ui.horizontal_wrapped(|ui| {
            let pending =
                env.capturing.filter(|(target, _)| **target == HotkeyTarget::Clicker).map(|(_, pending)| pending);
            match widgets::hotkey_button(ui, &config.hotkey, pending) {
                HotkeyAction::Capture => {
                    events.push(ClickerEvent::Request(UiRequest::CaptureHotkey(HotkeyTarget::Clicker)))
                }
                HotkeyAction::Cancel => events.push(ClickerEvent::Request(UiRequest::CancelCapture)),
                HotkeyAction::Clear => config.hotkey = Hotkey::none(),
                HotkeyAction::Set(hotkey) => config.hotkey = hotkey,
                HotkeyAction::None => {}
            }
            widgets::segmented(ui, &mut config.activation, &[(StartMode::Toggle, "Toggle"), (StartMode::Hold, "Hold")]);
        });
        let explanation = match config.activation {
            StartMode::Hold => "Clicks for as long as the hotkey is held down.",
            _ => "Press once to start, again to stop.",
        };
        widgets::note(ui, explanation);
        if config.hotkey.is_none() {
            widgets::notice(
                ui,
                icon::WARNING,
                "Without a hotkey the clicker can only be stopped here, or with the stop-everything hotkey.",
                theme::warning(),
            );
        }
    });
}

fn footer(ui: &mut Ui, config: &AutoClickerConfig, env: &ClickerEnv<'_>, events: &mut Vec<ClickerEvent>) {
    if let Some(error) = &env.error {
        widgets::notice(ui, icon::WARNING, error, theme::danger());
    }
    let hotkey = if config.hotkey.is_none() { String::new() } else { format!("  ({})", config.hotkey.label()) };
    // Two buttons, not one that turns into "Stop": clicks landing on the
    // button that was just pressed must not stop the clicker.
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        let width = ((ui.available_width() - gap) / 2.0).max(60.0);
        let start = ui.add_enabled_ui(!env.running, |ui| {
            widgets::accent_button(ui, format!("{}  Start{hotkey}", icon::PLAY), theme::accent(), vec2(width, 42.0))
        });
        if start.inner.on_hover_text("Start clicking").clicked() {
            events.push(ClickerEvent::Start);
        }
        let stop = ui.add_enabled_ui(env.running, |ui| {
            let color = if env.running { theme::danger() } else { theme::bg_widget() };
            widgets::accent_button(ui, format!("{}  Stop{hotkey}", icon::STOP), color, vec2(width, 42.0))
        });
        if stop.inner.on_hover_text("Stop clicking").clicked() {
            events.push(ClickerEvent::Stop);
        }
    });
    let status = match (env.running, env.clicks) {
        (true, n) => format!("Clicking…  {n} so far"),
        (false, 0) => config.summary(),
        (false, n) => format!("Stopped after {n} clicks"),
    };
    ui.vertical_centered(|ui| {
        let color = if env.running { theme::success() } else { theme::text_faint() };
        ui.label(RichText::new(status).size(11.5).color(color));
    });
}
