//! The inspector: settings of the selected block.
//!
//! The form is generated from the block's definition – one editor per
//! parameter, chosen by the parameter's [`ParamKind`]. A node type therefore
//! gets a complete settings panel without any UI code of its own; add-on nodes
//! will get theirs the same way.

use egui::{DragValue, RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use magi_core::action::{Action, PressMode};
use magi_core::assets::AssetKind;
use magi_core::geom::{Point, Rect};
use magi_core::graph::{Node, NodeId};
use magi_core::hotkey::Hotkey;
use magi_core::input::{Key, MouseButton};
use magi_core::nodes::flow::Case;
use magi_core::nodes::{EngineSupport, ParamDef, ParamKind, Registry};
use magi_core::project::Project;
use magi_core::text::{format_hex_color, parse_hex_color};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::graph::node_icon;
use super::images::{ImageCache, fit_size};
use super::widgets::{self, HotkeyAction};
use super::{HotkeyTarget, SnipMode, SnipTarget, UiRequest, recording_editor};
use crate::theme;

pub struct InspectorEnv<'a> {
    pub registry: &'a Registry,
    pub images: &'a mut ImageCache,
    /// The hotkey being rebound right now, and the chord pressed so far.
    pub capturing: Option<(&'a HotkeyTarget, &'a Hotkey)>,
    pub recording: bool,
}

/// Reads a parameter as a typed value, falling back to its default.
fn read<T: DeserializeOwned + Default>(node: &Node, param: &ParamDef) -> T {
    node.param(&param.key).or_else(|| T::deserialize(&param.default).ok()).unwrap_or_default()
}

/// Shows the settings of one block. Returns anything that needs the rest of the app.
pub fn show(ui: &mut Ui, project: &mut Project, node_id: NodeId, env: &mut InspectorEnv<'_>) -> Vec<UiRequest> {
    let mut requests = Vec::new();
    let Some(node) = project.graph.node(node_id) else {
        ui.weak("This block no longer exists.");
        return requests;
    };
    let Some(def) = env.registry.get(&node.kind).cloned() else {
        ui.colored_label(theme::WARNING, format!("Unknown block type “{}”", node.kind));
        ui.weak(
            "It probably comes from an add-on that is not installed. It is kept as it is, but cannot be edited or run.",
        );
        return requests;
    };

    // ---- header -----------------------------------------------------------
    let accent = theme::category_color(def.category);
    ui.horizontal(|ui| {
        ui.label(RichText::new(node_icon(&def.id)).size(20.0).color(accent));
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(&def.name).strong().size(15.0));
            ui.label(RichText::new(def.category.label()).size(11.0).color(accent));
        });
    });
    ui.label(RichText::new(&def.description).size(12.0).color(theme::TEXT_DIM));
    if def.engines == EngineSupport::PythonOnly {
        ui.label(
            RichText::new(format!("{}  Runs on the Python engine only", icon::INFO)).size(11.5).color(theme::WARNING),
        );
    }
    ui.add_space(4.0);

    let node = project.graph.node_mut(node_id).expect("checked above");
    ui.horizontal(|ui| {
        widgets::caption(ui, "Name");
        ui.add(egui::TextEdit::singleline(&mut node.label).hint_text(&def.name).desired_width(f32::INFINITY));
    });
    ui.checkbox(&mut node.enabled, "Enabled").on_hover_text("A disabled block is skipped when the macro runs.");
    ui.separator();

    // ---- parameters -------------------------------------------------------
    let mut prune = false;
    for param in &def.params {
        let visible = project.graph.node(node_id).is_some_and(|node| param.is_visible(&node.params));
        if !visible {
            continue;
        }
        ui.push_id(&param.key, |ui| {
            prune |= edit_param(ui, project, node_id, param, env, &mut requests);
        });
        ui.add_space(3.0);
    }
    if prune {
        // A case was removed: drop the wires that went to its ports.
        project.graph.prune(env.registry);
    }
    requests
}

/// One parameter's caption and editor. Returns true if ports may have changed.
fn edit_param(
    ui: &mut Ui,
    project: &mut Project,
    node_id: NodeId,
    param: &ParamDef,
    env: &mut InspectorEnv<'_>,
    requests: &mut Vec<UiRequest>,
) -> bool {
    let Some(node) = project.graph.node_mut(node_id) else { return false };
    let key = param.key.as_str();
    if !matches!(param.kind, ParamKind::Bool) {
        widgets::caption_with_help(ui, &param.label, &param.help);
    }

    match &param.kind {
        ParamKind::Bool => {
            let mut value: bool = read(node, param);
            let response = ui.checkbox(&mut value, &param.label);
            let response = if param.help.is_empty() { response } else { response.on_hover_text(&param.help) };
            if response.changed() {
                node.set_param(key, value);
            }
        }
        ParamKind::Int { min, max, unit } => {
            let mut value: i64 = read(node, param);
            let field = DragValue::new(&mut value).range(*min..=*max).suffix(pad_unit(unit));
            if ui.add(field).changed() {
                node.set_param(key, value);
            }
        }
        ParamKind::Float { min, max, unit } => {
            let mut value: f64 = read(node, param);
            let field =
                DragValue::new(&mut value).range(*min..=*max).speed(0.02).max_decimals(2).suffix(pad_unit(unit));
            if ui.add(field).changed() {
                node.set_param(key, value);
            }
        }
        ParamKind::Duration => {
            let mut value: f64 = read(node, param);
            if widgets::duration_edit(ui, &mut value).changed() {
                node.set_param(key, value);
            }
        }
        ParamKind::Percent => {
            let mut percent = read::<f64>(node, param) * 100.0;
            let slider = egui::Slider::new(&mut percent, 0.0..=100.0).fixed_decimals(0).suffix("%");
            if ui.add(slider).changed() {
                node.set_param(key, (percent / 100.0).clamp(0.0, 1.0));
            }
        }
        ParamKind::Text { multiline } => {
            let mut value: String = read(node, param);
            let field = if *multiline {
                egui::TextEdit::multiline(&mut value).desired_rows(3)
            } else {
                egui::TextEdit::singleline(&mut value)
            };
            if ui.add(field.desired_width(f32::INFINITY)).changed() {
                node.set_param(key, value);
            }
        }
        ParamKind::Code { .. } => {
            let mut value: String = read(node, param);
            let field =
                egui::TextEdit::multiline(&mut value).code_editor().desired_rows(12).desired_width(f32::INFINITY);
            if ui.add(field).changed() {
                node.set_param(key, value);
            }
        }
        ParamKind::Choice { options } => {
            let current: String = read(node, param);
            let shown = options.iter().find(|o| o.value == current).map_or(current.as_str(), |o| o.label.as_str());
            let mut picked = None;
            egui::ComboBox::from_id_salt("choice").width(ui.available_width() - 8.0).selected_text(shown).show_ui(
                ui,
                |ui| {
                    for option in options {
                        if ui.selectable_label(option.value == current, &option.label).clicked() {
                            picked = Some(option.value.clone());
                        }
                    }
                },
            );
            if let Some(value) = picked {
                node.set_param(key, value);
            }
        }
        ParamKind::Key => {
            let mut value = node.param::<Key>(key).unwrap_or(Key::Space);
            if widgets::key_picker(ui, "key", &mut value) {
                node.set_param(key, value);
            }
        }
        ParamKind::MouseButton => {
            let mut value = node.param::<MouseButton>(key).unwrap_or(MouseButton::Left);
            if mouse_button_picker(ui, "button", &mut value) {
                node.set_param(key, value);
            }
        }
        ParamKind::Hotkey => {
            let hotkey: Hotkey = read(node, param);
            let target = HotkeyTarget::Node(node_id, key.to_owned());
            let pending = env.capturing.filter(|(active, _)| **active == target).map(|(_, pending)| pending);
            match widgets::hotkey_button(ui, &hotkey, pending) {
                HotkeyAction::Capture => requests.push(UiRequest::CaptureHotkey(target)),
                HotkeyAction::Clear => node.set_param(key, Hotkey::none()),
                HotkeyAction::None => {}
            }
        }
        ParamKind::Point => {
            let mut value: Option<Point> = read(node, param);
            ui.horizontal(|ui| {
                match &mut value {
                    Some(point) => {
                        if widgets::point_edit(ui, point) {
                            node.set_param(key, *point);
                        }
                        if widgets::icon_button(ui, icon::X, "Clear").clicked() {
                            node.set_param(key, Value::Null);
                        }
                    }
                    None => {
                        ui.weak("Not set");
                    }
                }
                if ui.button(format!("{}  Pick", icon::CROSSHAIR)).clicked() {
                    requests.push(UiRequest::Snip(SnipMode::Point, SnipTarget::NodeParam(node_id, key.to_owned())));
                }
            });
        }
        ParamKind::Region => {
            let mut value: Option<Rect> = read(node, param);
            if let Some(region) = &mut value {
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    changed |= ui.add(DragValue::new(&mut region.x).prefix("X ")).changed();
                    changed |= ui.add(DragValue::new(&mut region.y).prefix("Y ")).changed();
                    changed |= ui.add(DragValue::new(&mut region.w).range(1..=100_000).prefix("W ")).changed();
                    changed |= ui.add(DragValue::new(&mut region.h).range(1..=100_000).prefix("H ")).changed();
                });
                if changed {
                    node.set_param(key, *region);
                }
            } else {
                ui.weak("Whole screen");
            }
            ui.horizontal(|ui| {
                if ui.button(format!("{}  Select area…", icon::SELECTION)).clicked() {
                    requests.push(UiRequest::Snip(SnipMode::Region, SnipTarget::NodeParam(node_id, key.to_owned())));
                }
                if value.is_some() && ui.button("Whole screen").clicked() {
                    node.set_param(key, Value::Null);
                }
            });
        }
        ParamKind::Color => {
            let text: String = read(node, param);
            let mut rgb = parse_hex_color(&text).unwrap_or([255, 0, 0]);
            ui.horizontal(|ui| {
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    node.set_param(key, format_hex_color(rgb));
                }
                let mut hex = text.clone();
                if ui.add(egui::TextEdit::singleline(&mut hex).desired_width(74.0)).changed() {
                    node.set_param(key, hex);
                }
                if ui
                    .button(format!("{}  Pick", icon::EYEDROPPER))
                    .on_hover_text("Take the colour of a pixel on screen")
                    .clicked()
                {
                    requests.push(UiRequest::Snip(SnipMode::Color, SnipTarget::NodeParam(node_id, key.to_owned())));
                }
            });
        }
        ParamKind::Image => edit_image(ui, project, node_id, key, env, requests),
        ParamKind::Recording => edit_recording(ui, project, node_id, key, env, requests),
        ParamKind::Actions => edit_actions(ui, node, key, requests),
        ParamKind::Cases => return edit_cases(ui, node, key),
    }
    false
}

fn pad_unit(unit: &str) -> String {
    if unit.is_empty() { String::new() } else { format!(" {unit}") }
}

fn mouse_button_picker(ui: &mut Ui, id: &str, button: &mut MouseButton) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).width(90.0).selected_text(button.label()).show_ui(ui, |ui| {
        for &option in MouseButton::ALL {
            if ui.selectable_label(option == *button, option.label()).clicked() {
                *button = option;
                changed = true;
            }
        }
    });
    changed
}

/// Lets the user pick one of the project's assets of a kind, and rename it.
fn asset_picker(ui: &mut Ui, project: &mut Project, node_id: NodeId, key: &str, kind: AssetKind, none_label: &str) {
    let current: String = project.graph.node(node_id).and_then(|n| n.param(key)).unwrap_or_default();
    let shown = project.assets.get(&current).map_or(none_label.to_owned(), |asset| asset.name.clone());
    let mut picked: Option<String> = None;
    egui::ComboBox::from_id_salt(("asset", key)).width(ui.available_width() - 8.0).selected_text(shown).show_ui(
        ui,
        |ui| {
            if ui.selectable_label(current.is_empty(), none_label).clicked() {
                picked = Some(String::new());
            }
            for asset in project.assets.of_kind(kind) {
                if ui.selectable_label(asset.id == current, &asset.name).clicked() {
                    picked = Some(asset.id.clone());
                }
            }
        },
    );
    if let Some(id) = picked
        && let Some(node) = project.graph.node_mut(node_id)
    {
        node.set_param(key, id);
    }
    if let Some(asset) = project.assets.get(&current) {
        let mut name = asset.name.clone();
        ui.horizontal(|ui| {
            widgets::caption(ui, if kind == AssetKind::Image { "Image name" } else { "Recording name" });
            if ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY)).changed()
                && let Some(asset) = project.assets.get_mut(&current)
            {
                asset.name = name;
            }
        });
    }
}

fn edit_image(
    ui: &mut Ui,
    project: &mut Project,
    node_id: NodeId,
    key: &str,
    env: &mut InspectorEnv<'_>,
    requests: &mut Vec<UiRequest>,
) {
    let current: String = project.graph.node(node_id).and_then(|n| n.param(key)).unwrap_or_default();
    if let Some(texture) = env.images.get(ui.ctx(), &project.assets, &current) {
        egui::Frame::new().fill(theme::BG_INPUT).corner_radius(theme::RADIUS).inner_margin(6).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                let size = fit_size(&texture, vec2(ui.available_width(), 120.0), 6.0);
                ui.image((texture.id(), size));
                let [w, h] = texture.size();
                ui.label(RichText::new(format!("{w} × {h} px")).size(11.0).color(theme::TEXT_FAINT));
            });
        });
    }
    ui.horizontal(|ui| {
        if ui
            .button(format!("{}  Capture from screen", icon::CAMERA))
            .on_hover_text("Drag a box around the thing to look for")
            .clicked()
        {
            requests.push(UiRequest::Snip(SnipMode::Image, SnipTarget::NodeParam(node_id, key.to_owned())));
        }
        if ui.button(format!("{}  Load file…", icon::FOLDER_OPEN)).clicked() {
            requests.push(UiRequest::LoadImage(node_id, key.to_owned()));
        }
    });
    asset_picker(ui, project, node_id, key, AssetKind::Image, "No image");
}

fn edit_recording(
    ui: &mut Ui,
    project: &mut Project,
    node_id: NodeId,
    key: &str,
    env: &mut InspectorEnv<'_>,
    requests: &mut Vec<UiRequest>,
) {
    asset_picker(ui, project, node_id, key, AssetKind::Recording, "No recording");
    let label =
        if env.recording { format!("{}  Stop recording", icon::STOP) } else { format!("{}  Record new", icon::RECORD) };
    if ui.button(label).on_hover_text("Records what you do and links the result to this block").clicked() {
        requests.push(UiRequest::RecordInto(node_id, key.to_owned()));
    }
    let current: String = project.graph.node(node_id).and_then(|n| n.param(key)).unwrap_or_default();
    if let Some(recording) = project.assets.recording(&current).cloned() {
        ui.add_space(4.0);
        ui.push_id("inspector-recording", |ui| {
            if let Some(edit) = recording_editor::show(ui, &recording, 260.0)
                && let Some(target) = project.assets.recording_mut(&current)
            {
                edit.apply(target);
            }
        });
    }
}

fn press_mode_picker(ui: &mut Ui, mode: &mut PressMode) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt("mode").width(92.0).selected_text(mode.label()).show_ui(ui, |ui| {
        for option in PressMode::ALL {
            if ui.selectable_label(option == *mode, option.label()).clicked() {
                *mode = option;
                changed = true;
            }
        }
    });
    changed
}

/// The list of inputs an Input block performs together.
fn edit_actions(ui: &mut Ui, node: &mut Node, key: &str, requests: &mut Vec<UiRequest>) {
    let mut actions: Vec<Action> = node.param(key).unwrap_or_default();
    let mut changed = false;
    let mut remove = None;
    let mut swap = None;
    let count = actions.len();

    for (index, action) in actions.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            egui::Frame::new().fill(theme::BG_RAISED).corner_radius(theme::RADIUS).inner_margin(7).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let mut kind = action.kind();
                    egui::ComboBox::from_id_salt("kind").width(118.0).selected_text(Action::kind_label(kind)).show_ui(
                        ui,
                        |ui| {
                            for option in Action::KINDS {
                                ui.selectable_value(&mut kind, option, Action::kind_label(option));
                            }
                        },
                    );
                    if kind != action.kind()
                        && let Some(fresh) = Action::default_of(kind)
                    {
                        *action = fresh;
                        changed = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon_button(ui, icon::TRASH, "Remove this input").clicked() {
                            remove = Some(index);
                        }
                        if index + 1 < count && widgets::icon_button(ui, icon::ARROW_DOWN, "Do later").clicked() {
                            swap = Some((index, index + 1));
                        }
                        if index > 0 && widgets::icon_button(ui, icon::ARROW_UP, "Do earlier").clicked() {
                            swap = Some((index, index - 1));
                        }
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    match action {
                        Action::Key { key, mode } => {
                            changed |= widgets::key_picker(ui, "key", key);
                            changed |= press_mode_picker(ui, mode);
                        }
                        Action::Mouse { button, mode, clicks, at } => {
                            changed |= mouse_button_picker(ui, "button", button);
                            changed |= press_mode_picker(ui, mode);
                            if *mode == PressMode::Tap {
                                changed |= ui
                                    .add(DragValue::new(clicks).range(1..=10).suffix("×"))
                                    .on_hover_text("Clicks: 2 is a double-click")
                                    .changed();
                            }
                            ui.end_row();
                            let mut fixed = at.is_some();
                            if ui
                                .checkbox(&mut fixed, "At")
                                .on_hover_text("Click at a fixed position instead of where the pointer is")
                                .changed()
                            {
                                *at = fixed.then(Point::default);
                                changed = true;
                            }
                            if let Some(point) = at {
                                changed |= widgets::point_edit(ui, point);
                                if ui.button(icon::CROSSHAIR).on_hover_text("Pick the position on screen").clicked() {
                                    requests.push(UiRequest::Snip(SnipMode::Point, SnipTarget::Action(node.id, index)));
                                }
                            }
                        }
                        Action::Move { x, y, relative, duration_ms } => {
                            let mut point = Point::new(*x, *y);
                            if widgets::point_edit(ui, &mut point) {
                                (*x, *y) = (point.x, point.y);
                                changed = true;
                            }
                            if !*relative
                                && ui.button(icon::CROSSHAIR).on_hover_text("Pick the position on screen").clicked()
                            {
                                requests.push(UiRequest::Snip(SnipMode::Point, SnipTarget::Action(node.id, index)));
                            }
                            ui.end_row();
                            changed |= ui
                                .checkbox(relative, "Relative")
                                .on_hover_text("Move by this much from the current position")
                                .changed();
                            let mut glide = *duration_ms as f64;
                            ui.label(RichText::new("over").color(theme::TEXT_DIM));
                            if widgets::duration_edit(ui, &mut glide).changed() {
                                *duration_ms = glide.round() as u64;
                                changed = true;
                            }
                        }
                        Action::Scroll { dx, dy } => {
                            changed |= ui
                                .add(DragValue::new(dy).speed(0.1).prefix("↕ "))
                                .on_hover_text("Notches up (+) or down (−)")
                                .changed();
                            changed |= ui
                                .add(DragValue::new(dx).speed(0.1).prefix("↔ "))
                                .on_hover_text("Notches right (+) or left (−)")
                                .changed();
                        }
                        Action::Text { text } => {
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(text)
                                        .hint_text("Text to type")
                                        .desired_width(f32::INFINITY),
                                )
                                .changed();
                        }
                    }
                });
            });
        });
    }

    let mut added = None;
    ui.menu_button(format!("{}  Add input", icon::PLUS), |ui| {
        ui.label(RichText::new("Done at the same time as the others").small().weak());
        for kind in Action::KINDS {
            if ui.button(Action::kind_label(kind)).clicked() {
                added = Action::default_of(kind);
                ui.close();
            }
        }
    });

    if let Some(action) = added {
        actions.push(action);
        changed = true;
    }
    if let Some(index) = remove {
        actions.remove(index);
        changed = true;
    }
    if let Some((a, b)) = swap {
        actions.swap(a, b);
        changed = true;
    }
    if changed {
        node.set_param(key, actions);
    }
}

/// The cases of a Decision block. Returns true if a case was removed.
fn edit_cases(ui: &mut Ui, node: &mut Node, key: &str) -> bool {
    let mut cases: Vec<Case> = node.param(key).unwrap_or_default();
    let mut changed = false;
    let mut remove = None;
    let mut swap = None;
    let count = cases.len();

    for (index, case) in cases.iter_mut().enumerate() {
        ui.push_id(&case.id, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new(format!("{}.", index + 1)).color(theme::TEXT_FAINT));
                let width = (ui.available_width() - 84.0).max(60.0);
                changed |= ui.add(egui::TextEdit::singleline(&mut case.label).desired_width(width)).changed();
                if index > 0 && widgets::icon_button(ui, icon::ARROW_UP, "Check earlier").clicked() {
                    swap = Some((index, index - 1));
                }
                if index + 1 < count && widgets::icon_button(ui, icon::ARROW_DOWN, "Check later").clicked() {
                    swap = Some((index, index + 1));
                }
                if widgets::icon_button(ui, icon::TRASH, "Remove this case and its wires").clicked() {
                    remove = Some(index);
                }
            });
        });
    }
    ui.label(
        RichText::new("Checked top to bottom; the first true case is followed.").size(11.0).color(theme::TEXT_FAINT),
    );

    if ui.button(format!("{}  Add case", icon::PLUS)).clicked() {
        let mut params = magi_core::nodes::flow::DecisionParams { cases: std::mem::take(&mut cases) };
        params.add_case();
        cases = params.cases;
        changed = true;
    }
    if let Some((a, b)) = swap {
        cases.swap(a, b);
        changed = true;
    }
    let removed = remove.is_some();
    if let Some(index) = remove {
        cases.remove(index);
        changed = true;
    }
    if changed {
        node.set_param(key, cases);
    }
    removed
}
