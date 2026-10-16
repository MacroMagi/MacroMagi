//! The inspector: settings of the selected block.
//!
//! The form is generated from the block's definition – one editor per
//! parameter, chosen by the parameter's [`ParamKind`]. A node type therefore
//! gets a complete settings panel without any UI code of its own; add-on nodes
//! will get theirs the same way.
//!
//! Every setting that differs from its default gets a small reset button, and
//! settings that can follow a variable get a link button: a linked setting
//! shows which variable it follows instead of an editor.

use egui::{DragValue, RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use magi_core::action::{Action, PressMode};
use magi_core::assets::AssetKind;
use magi_core::geom::{Displays, Point, Rect};
use magi_core::graph::{Node, NodeId};
use magi_core::hotkey::Hotkey;
use magi_core::input::{Key, MouseButton};
use magi_core::nodes::flow::{Case, DefineParams};
use magi_core::nodes::{EngineSupport, ParamDef, ParamKind, Registry, ids};
use magi_core::project::Project;
use magi_core::text::{format_hex_color, parse_hex_color};
use magi_core::variables::{self, Variable};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::graph::node_icon;
use super::images::{ImageCache, fit_size};
use super::widgets::{self, HotkeyAction};
use super::{HotkeyTarget, SnipMode, SnipTarget, UiRequest, clicker_form, options, recording_editor};
use crate::theme;

pub struct InspectorEnv<'a> {
    pub registry: &'a Registry,
    pub images: &'a mut ImageCache,
    /// The hotkey being rebound right now, and the chord pressed so far.
    pub capturing: Option<(&'a HotkeyTarget, &'a Hotkey)>,
    pub recording: bool,
    pub displays: &'a Displays,
    /// The variables' values in the running macro, if one runs.
    pub live: &'a Map<String, Value>,
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
        ui.colored_label(theme::warning(), format!("Unknown block type “{}”", node.kind));
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
    ui.add(egui::Label::new(RichText::new(&def.description).size(12.0).color(theme::text_dim())).wrap());
    if def.engines == EngineSupport::PythonOnly {
        ui.label(
            RichText::new(format!("{}  Runs on the Python engine only", icon::INFO)).size(11.5).color(theme::warning()),
        );
    }
    let groups: Vec<(String, bool)> = project
        .graph
        .groups()
        .iter()
        .filter(|g| g.nodes.contains(&node_id))
        .map(|g| (g.name.clone(), g.enabled))
        .collect();
    for (name, enabled) in groups {
        let text = if enabled {
            format!("{}  In the group “{name}”", icon::BOUNDING_BOX)
        } else {
            format!("{}  In the group “{name}”, which is turned off: this block is skipped", icon::BOUNDING_BOX)
        };
        let color = if enabled { theme::text_faint() } else { theme::warning() };
        ui.add(egui::Label::new(RichText::new(text).size(11.5).color(color)).wrap());
    }
    ui.add_space(4.0);

    let node = project.graph.node_mut(node_id).expect("checked above");
    ui.horizontal(|ui| {
        widgets::caption(ui, "Name");
        ui.add(egui::TextEdit::singleline(&mut node.label).hint_text(&def.name).desired_width(f32::INFINITY));
    });
    ui.checkbox(&mut node.enabled, "Enabled").on_hover_text("A disabled block is skipped when the macro runs.");
    if def.id == ids::RATE {
        ui.add_space(2.0);
        let button = egui::Button::new(RichText::new(format!("{}  VARIABLE RATE EDITOR", icon::METRONOME)).strong())
            .min_size(vec2(ui.available_width(), 28.0));
        if ui.add(button).on_hover_text("The steps and their timing in a larger window").clicked() {
            requests.push(UiRequest::EditRate(node_id));
        }
    }
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

/// One setting of a block, with its caption, reset and link buttons, for
/// windows that show a block's settings outside the inspector.
pub fn edit_setting(
    ui: &mut Ui,
    project: &mut Project,
    node_id: NodeId,
    key: &str,
    env: &mut InspectorEnv<'_>,
) -> Vec<UiRequest> {
    let mut requests = Vec::new();
    let def = project.graph.node(node_id).and_then(|node| env.registry.get(&node.kind)).cloned();
    if let Some(param) = def.as_ref().and_then(|def| def.params.iter().find(|p| p.key == key)) {
        ui.push_id(key, |ui| {
            edit_param(ui, project, node_id, param, env, &mut requests);
        });
    }
    requests
}

/// What the link menu next to a setting asked for.
enum Link {
    Follow(String),
    /// Make a new variable holding the setting's value, and follow it.
    NewVariable,
    Unfollow,
}

/// The kinds of setting a reset button would do more harm than good on:
/// pictures, recordings and sounds (resetting would just forget them) and
/// lists edited item by item.
fn resettable(kind: &ParamKind) -> bool {
    !matches!(
        kind,
        ParamKind::Image
            | ParamKind::Recording
            | ParamKind::Audio
            | ParamKind::Actions
            | ParamKind::Cases
            | ParamKind::Clicker
    )
}

/// A name for a new variable that no other variable has.
fn unique_variable_name(project: &Project, base: &str) -> String {
    let base = base.trim();
    let base = if base.is_empty() { "variable" } else { base };
    let taken = |name: &str| project.variables.iter().any(|v| v.name == name);
    if !taken(base) {
        return base.to_owned();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|name| !taken(name))
        .expect("an unbounded range always yields a free name")
}

/// The link button: follow one of the variables, make a new one, or stop following.
fn link_menu(ui: &mut Ui, variables: &[Variable], bound: Option<&str>) -> Option<Link> {
    let mut chosen = None;
    let (glyph, color) =
        if bound.is_some() { (icon::LINK, theme::accent_bright()) } else { (icon::LINK, theme::text_faint()) };
    let button =
        egui::Button::new(RichText::new(glyph).size(12.5).color(color)).frame(false).min_size(vec2(18.0, 18.0));
    egui::containers::menu::MenuButton::from_button(button)
        .ui(ui, |ui| {
            ui.set_min_width(190.0);
            ui.label(RichText::new("Follow a variable").size(11.0).color(theme::text_faint()));
            if variables.is_empty() {
                ui.weak("No variables yet");
            }
            for variable in variables {
                if ui.selectable_label(bound == Some(variable.name.as_str()), &variable.name).clicked() {
                    chosen = Some(Link::Follow(variable.name.clone()));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(format!("{}  New variable from this setting", icon::PLUS)).clicked() {
                chosen = Some(Link::NewVariable);
                ui.close();
            }
            if bound.is_some() && ui.button(format!("{}  Stop following", icon::LINK_BREAK)).clicked() {
                chosen = Some(Link::Unfollow);
                ui.close();
            }
        })
        .0
        .on_hover_text(match bound {
            Some(name) => format!("Follows the variable “{name}”"),
            None => "Let this setting follow a variable, so blocks, scripts and Set Variable can change it".to_owned(),
        });
    chosen
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
    let Some(node) = project.graph.node(node_id) else { return false };
    let key = param.key.as_str();
    let current = node.params.get(key).cloned().unwrap_or_else(|| param.default.clone());
    let bound = variables::bound_name(&current).map(str::to_owned);
    let changed = resettable(&param.kind) && current != param.default;
    let is_bool = matches!(param.kind, ParamKind::Bool) && bound.is_none();

    // The caption row: the label (or the checkbox), then reset and link.
    let mut checked = is_bool && variables::truthy(&current);
    let mut flipped = false;
    let mut reset = false;
    let mut link = None;
    ui.horizontal(|ui| {
        if is_bool {
            let response = ui.checkbox(&mut checked, &param.label);
            let response = if param.help.is_empty() { response } else { response.on_hover_text(&param.help) };
            flipped = response.changed();
        } else {
            widgets::caption_with_help(ui, &param.label, &param.help);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            reset = widgets::reset_button(ui, changed);
            if param.kind.bindable() {
                link = link_menu(ui, &project.variables, bound.as_deref());
            }
        });
    });

    if reset {
        if let Some(node) = project.graph.node_mut(node_id) {
            node.params.insert(key.to_owned(), param.default.clone());
        }
        return false;
    }
    if let Some(link) = link {
        let value = match link {
            Link::Follow(name) => variables::binding(&name),
            Link::NewVariable => {
                let name = unique_variable_name(project, &param.label.to_lowercase());
                let start = if bound.is_some() { param.default.clone() } else { current.clone() };
                project.variables.push(Variable { value: start, ..Variable::kept(name.clone()) });
                variables::binding(&name)
            }
            Link::Unfollow => {
                let start = bound.as_deref().and_then(|name| project.variables.iter().find(|v| v.name == name));
                start.map_or_else(|| param.default.clone(), |v| variables::coerce(&v.value, &param.default))
            }
        };
        if let Some(node) = project.graph.node_mut(node_id) {
            node.params.insert(key.to_owned(), value);
        }
        return false;
    }
    if flipped {
        if let Some(node) = project.graph.node_mut(node_id) {
            node.set_param(key, checked);
        }
        return false;
    }
    if let Some(name) = bound {
        bound_chip(ui, project, &name, env.live);
        return false;
    }
    if is_bool {
        return false;
    }

    // Lists some editors need, gathered before the block is borrowed for editing.
    let definitions: Vec<String> = match param.kind {
        ParamKind::Definition => project
            .graph
            .nodes()
            .filter(|n| n.kind == ids::DEFINE)
            .map(|n| n.params_or_default::<DefineParams>().name)
            .filter(|name| !name.trim().is_empty())
            .collect(),
        _ => Vec::new(),
    };
    let blocks: Vec<(NodeId, String)> = match param.kind {
        ParamKind::Block => project
            .graph
            .nodes()
            .filter(|n| n.id != node_id && env.registry.ports(n).flow_in)
            .map(|n| (n.id, format!("#{}  {}", n.id, env.registry.title(n))))
            .collect(),
        _ => Vec::new(),
    };

    let Some(node) = project.graph.node_mut(node_id) else { return false };
    match &param.kind {
        ParamKind::Bool => {}
        ParamKind::Int { min, max, unit } => {
            let mut value: i64 = read(node, param);
            let field = DragValue::new(&mut value).range(*min..=*max).suffix(pad_unit(unit));
            if ui.add(field).changed() {
                node.set_param(key, value);
            }
        }
        ParamKind::Float { min, max, unit } => {
            let mut value: f64 = read(node, param);
            // Dragging covers a narrow range finely and a wide one quickly.
            let range = max - min;
            let speed = if range > 1e6 { 0.1 } else { (range / 1000.0).clamp(0.02, 5.0) };
            let field =
                DragValue::new(&mut value).range(*min..=*max).speed(speed).max_decimals(2).suffix(pad_unit(unit));
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
            // The window to switch to can be picked from the running programs.
            let pick_program = node.kind == ids::FOCUS_WINDOW && key == "target";
            ui.horizontal(|ui| {
                let width = if pick_program { ui.available_width() - 34.0 } else { ui.available_width() };
                let field = if *multiline {
                    egui::TextEdit::multiline(&mut value).desired_rows(3)
                } else {
                    egui::TextEdit::singleline(&mut value)
                };
                if ui.add(field.desired_width(width)).changed() {
                    node.set_param(key, value.clone());
                }
                if pick_program && let Some(name) = options::program_picker(ui, "window-program") {
                    node.set_param(key, name);
                    node.set_param("match_by", "process");
                }
            });
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
            let mut value = node
                .param::<Key>(key)
                .or_else(|| serde_json::from_value(param.default.clone()).ok())
                .unwrap_or(Key::Space);
            if widgets::key_picker(ui, "key", &mut value) {
                node.set_param(key, value);
            }
        }
        ParamKind::MouseButton => {
            let mut value = node.param::<MouseButton>(key).unwrap_or(MouseButton::Left);
            if widgets::mouse_button_picker(ui, "button", &mut value) {
                node.set_param(key, value);
            }
        }
        ParamKind::Hotkey => {
            let hotkey: Hotkey = read(node, param);
            let target = HotkeyTarget::Node(node_id, key.to_owned());
            let pending = env.capturing.filter(|(active, _)| **active == target).map(|(_, pending)| pending);
            match widgets::hotkey_button(ui, &hotkey, pending) {
                HotkeyAction::Capture => requests.push(UiRequest::CaptureHotkey(target)),
                HotkeyAction::Cancel => requests.push(UiRequest::CancelCapture),
                HotkeyAction::Clear => node.set_param(key, Hotkey::none()),
                HotkeyAction::Set(hotkey) => node.set_param(key, hotkey),
                HotkeyAction::None => {}
            }
        }
        ParamKind::Point => {
            let mut value: Option<Point> = read(node, param);
            let displays = env.displays;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                match &mut value {
                    Some(point) => {
                        if widgets::point_edit(ui, point) {
                            node.set_param(key, *point);
                        }
                        let mut display = point.display;
                        if widgets::display_picker(ui, "display", &mut display, displays, true) {
                            node.set_param(key, widgets::point_on(*point, display, displays));
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
                let mut display = region.display;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    changed |= ui.add(DragValue::new(&mut region.x).prefix("X ")).changed();
                    changed |= ui.add(DragValue::new(&mut region.y).prefix("Y ")).changed();
                    changed |= ui.add(DragValue::new(&mut region.w).range(1..=100_000).prefix("W ")).changed();
                    changed |= ui.add(DragValue::new(&mut region.h).range(1..=100_000).prefix("H ")).changed();
                    if widgets::display_picker(ui, "display", &mut display, env.displays, true) {
                        *region = widgets::rect_on(*region, display, env.displays);
                        changed = true;
                    }
                });
                if changed {
                    node.set_param(key, *region);
                }
            } else {
                ui.weak("The whole display");
            }
            ui.horizontal(|ui| {
                if ui.button(format!("{}  Select area…", icon::SELECTION)).clicked() {
                    requests.push(UiRequest::Snip(SnipMode::Region, SnipTarget::NodeParam(node_id, key.to_owned())));
                }
                if value.is_some() && ui.button("Whole display").clicked() {
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
        ParamKind::Display => {
            let mut display = Some(read::<u8>(node, param));
            if widgets::display_picker(ui, "display", &mut display, env.displays, false) {
                node.set_param(key, display.unwrap_or(0));
            }
            ui.label(RichText::new(env.displays.label(display.unwrap_or(0))).size(11.0).color(theme::text_faint()));
        }
        ParamKind::Variable => variable_picker(ui, project, node_id, key),
        ParamKind::Definition => {
            let current: String = read(node, param);
            let shown = if current.is_empty() { "Not chosen".to_owned() } else { current.clone() };
            egui::ComboBox::from_id_salt("definition").width(ui.available_width() - 8.0).selected_text(shown).show_ui(
                ui,
                |ui| {
                    if definitions.is_empty() {
                        ui.weak("No Definition blocks yet – add one to build a function.");
                    }
                    for name in &definitions {
                        if ui.selectable_label(*name == current, name).clicked() {
                            node.set_param(key, name);
                        }
                    }
                },
            );
            if !current.is_empty() && !definitions.contains(&current) {
                widgets::notice(ui, icon::WARNING, "No Definition block has this name.", theme::warning());
            }
        }
        ParamKind::Block => {
            let current: Option<NodeId> = read(node, param);
            let shown = current
                .and_then(|id| blocks.iter().find(|(b, _)| *b == id))
                .map_or_else(|| "Not chosen".to_owned(), |(_, title)| title.clone());
            egui::ComboBox::from_id_salt("block").width(ui.available_width() - 8.0).selected_text(shown).show_ui(
                ui,
                |ui| {
                    if ui.selectable_label(current.is_none(), "Not chosen").clicked() {
                        node.set_param(key, Value::Null);
                    }
                    for (id, title) in &blocks {
                        if ui.selectable_label(current == Some(*id), title).clicked() {
                            node.set_param(key, *id);
                        }
                    }
                },
            );
        }
        ParamKind::Images => super::lists::edit_images(ui, project, node_id, env.images, requests),
        ParamKind::Sounds => super::lists::edit_sounds(ui, project, node_id, requests),
        ParamKind::AudioDevice => {
            let current: String = read(node, param);
            let shown = if current.is_empty() { "System default".to_owned() } else { current.clone() };
            let names = crate::displays::microphones();
            egui::ComboBox::from_id_salt("device")
                .width(ui.available_width() - 8.0)
                .selected_text(shown)
                .truncate()
                .show_ui(ui, |ui| {
                    if ui.selectable_label(current.is_empty(), "System default").clicked() {
                        node.set_param(key, "");
                    }
                    for name in &names {
                        if ui.selectable_label(*name == current, name).clicked() {
                            node.set_param(key, name);
                        }
                    }
                    if names.is_empty() {
                        ui.weak("No microphones found");
                    }
                });
            if !current.is_empty() && !names.contains(&current) {
                widgets::notice(ui, icon::WARNING, "This microphone is not connected.", theme::warning());
            }
        }
        ParamKind::Model => edit_model(ui, project, node_id, key, requests),
        ParamKind::ModelSettings => edit_model_settings(ui, project, node_id, key),
        ParamKind::Image => edit_image(ui, project, node_id, key, env, requests),
        ParamKind::Recording => edit_recording(ui, project, node_id, key, env, requests),
        ParamKind::Audio => edit_sound(ui, project, node_id, key, requests),
        ParamKind::Actions => edit_actions(ui, node, key, env.displays, requests),
        ParamKind::Cases => return edit_cases(ui, node, key),
        ParamKind::Clicker => {
            let original: magi_core::autoclicker::ClickerSettings = read(node, param);
            let mut settings = original.clone();
            requests.extend(clicker_form::show(
                ui,
                &mut settings,
                "node-clicker",
                SnipTarget::ClickerNode(node_id),
                env.displays,
            ));
            if settings != original {
                node.set_param(key, settings);
            }
        }
    }
    false
}

/// In place of the editor of a setting that follows a variable.
fn bound_chip(ui: &mut Ui, project: &Project, name: &str, live: &Map<String, Value>) {
    let exists = project.variables.iter().find(|v| v.name == name);
    egui::Frame::new()
        .fill(theme::with_alpha(theme::accent(), 0.12))
        .stroke(egui::Stroke::new(1.0, theme::with_alpha(theme::accent(), 0.4)))
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{}  {name}", icon::LINK)).strong());
                match (live.get(name), exists) {
                    (Some(value), _) => {
                        ui.label(RichText::new(format!("= {}", variables::as_text(value))).color(theme::text_dim()));
                    }
                    (None, Some(variable)) => {
                        let text = format!("starts as {}", variables::as_text(&variable.value));
                        ui.label(RichText::new(text).color(theme::text_faint()));
                    }
                    (None, None) => {
                        ui.label(RichText::new("no such variable: the default is used").color(theme::warning()));
                    }
                }
            });
        });
}

/// Choosing one of the project's variables, or making a new one.
fn variable_picker(ui: &mut Ui, project: &mut Project, node_id: NodeId, key: &str) {
    let current: String = project.graph.node(node_id).and_then(|n| n.param(key)).unwrap_or_default();
    let new_id = ui.id().with("new-variable");
    let mut typing: Option<String> = ui.data(|d| d.get_temp(new_id));
    let mut picked = None;
    ui.horizontal(|ui| {
        let shown = if current.is_empty() { "Not chosen".to_owned() } else { current.clone() };
        egui::ComboBox::from_id_salt("variable").width(ui.available_width() - 34.0).selected_text(shown).show_ui(
            ui,
            |ui| {
                if ui.selectable_label(current.is_empty(), "Not chosen").clicked() {
                    picked = Some(String::new());
                }
                for variable in &project.variables {
                    if ui.selectable_label(variable.name == current, &variable.name).clicked() {
                        picked = Some(variable.name.clone());
                    }
                }
            },
        );
        if widgets::icon_button(ui, icon::PLUS, "New variable").clicked() {
            typing = Some(String::new());
        }
    });
    if let Some(name) = &mut typing {
        let field = ui.add(
            egui::TextEdit::singleline(name)
                .hint_text("Name of the new variable, then Enter")
                .desired_width(f32::INFINITY),
        );
        if !field.has_focus() && !field.lost_focus() {
            field.request_focus();
        }
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if entered && !name.trim().is_empty() {
            let name = unique_variable_name(project, name);
            project.variables.push(Variable::kept(name.clone()));
            picked = Some(name);
            typing = None;
        } else if field.lost_focus() {
            typing = None;
        }
    }
    if !current.is_empty() && !project.variables.iter().any(|v| v.name == current) {
        widgets::notice(ui, icon::WARNING, "This variable does not exist any more.", theme::warning());
    }
    ui.data_mut(|d| {
        if let Some(name) = &typing {
            d.insert_temp(new_id, name.clone());
        } else {
            d.remove::<String>(new_id);
        }
    });
    if let Some(name) = picked
        && let Some(node) = project.graph.node_mut(node_id)
    {
        node.set_param(key, name);
    }
}

fn pad_unit(unit: &str) -> String {
    if unit.is_empty() { String::new() } else { format!(" {unit}") }
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
            widgets::caption(ui, "Name");
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
        egui::Frame::new().fill(theme::bg_input()).corner_radius(theme::RADIUS).inner_margin(6).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                let size = fit_size(&texture, vec2(ui.available_width(), 120.0), 6.0);
                ui.image((texture.id(), size));
                let [w, h] = texture.size();
                ui.label(RichText::new(format!("{w} × {h} px")).size(11.0).color(theme::text_faint()));
            });
        });
    }
    ui.horizontal_wrapped(|ui| {
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

fn edit_model(ui: &mut Ui, project: &mut Project, node_id: NodeId, key: &str, requests: &mut Vec<UiRequest>) {
    asset_picker(ui, project, node_id, key, AssetKind::Model, "No model");
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(format!("{}  Load .magic file…", icon::FOLDER_OPEN))
            .on_hover_text("A model saved from the Magic tab")
            .clicked()
        {
            requests.push(UiRequest::LoadModel(node_id, key.to_owned()));
        }
        let current: String = project.graph.node(node_id).and_then(|n| n.param(key)).unwrap_or_default();
        if project.assets.model(&current).is_some()
            && ui
                .button(format!("{}  Open in Magic", icon::MAGIC_WAND))
                .on_hover_text("See and change the model's setup in the Magic tab, then use “Use in Macro” again")
                .clicked()
        {
            requests.push(UiRequest::OpenModel(current));
        }
    });
}

/// The description inside a model asset, read once per asset.
fn model_description(project: &Project, id: &str) -> Option<magi_core::magic::MagicModel> {
    thread_local! {
        static READ: std::cell::RefCell<std::collections::HashMap<String, Option<magi_core::magic::MagicModel>>> =
            std::cell::RefCell::default();
    }
    READ.with(|read| {
        read.borrow_mut()
            .entry(id.to_owned())
            .or_insert_with(|| {
                let bytes = project.assets.model(id)?;
                magi_core::magic::MagicFile::from_bytes(bytes).ok().map(|file| file.model)
            })
            .clone()
    })
}

/// The model's adjustable settings, set for this block.
fn edit_model_settings(ui: &mut Ui, project: &mut Project, node_id: NodeId, key: &str) {
    let Some(node) = project.graph.node(node_id) else { return };
    let model_id: String = node.param("model").unwrap_or_default();
    let Some(model) = model_description(project, &model_id) else {
        ui.weak("Choose a model first.");
        return;
    };
    if model.settings.is_empty() {
        ui.weak("This model has no adjustable settings.");
        return;
    }
    let mut values: Map<String, Value> = node.param(key).unwrap_or_default();
    let mut changed = false;
    for setting in &model.settings {
        ui.push_id(&setting.key, |ui| {
            let current = values.get(&setting.key).and_then(Value::as_f64);
            ui.horizontal(|ui| {
                widgets::caption_with_help(ui, &setting.label, &setting.help);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::reset_button(ui, current.is_some()) {
                        values.remove(&setting.key);
                        changed = true;
                    }
                });
            });
            let mut value = current.unwrap_or(setting.value);
            let suffix = match setting.kind {
                magi_core::magic::SettingKind::Percent => "%",
                magi_core::magic::SettingKind::Milliseconds => " ms",
            };
            let (min, max) = (setting.min.min(setting.max), setting.max.max(setting.min));
            if ui.add(egui::Slider::new(&mut value, min..=max).max_decimals(0).suffix(suffix)).changed() {
                values.insert(setting.key.clone(), serde_json::json!(value));
                changed = true;
            }
        });
    }
    if changed && let Some(node) = project.graph.node_mut(node_id) {
        node.set_param(key, Value::Object(values));
    }
}

fn edit_sound(ui: &mut Ui, project: &mut Project, node_id: NodeId, key: &str, requests: &mut Vec<UiRequest>) {
    if ui
        .button(format!("{}  Load sound file…", icon::FOLDER_OPEN))
        .on_hover_text("A WAV, MP3, OGG or FLAC file with the sound to listen for")
        .clicked()
    {
        requests.push(UiRequest::LoadSound(node_id, key.to_owned()));
    }
    asset_picker(ui, project, node_id, key, AssetKind::Audio, "No sound");
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
            let response = recording_editor::controls(ui, &recording);
            if let Some(edit) = response.edit
                && let Some(target) = project.assets.recording_mut(&current)
            {
                edit.apply(target);
            }
            if response.open_editor {
                requests.push(UiRequest::EditRecording(current.clone()));
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

/// A list of inputs: the inputs an Input block performs together, or the
/// steps of a Rate block.
pub fn edit_actions(ui: &mut Ui, node: &mut Node, key: &str, displays: &Displays, requests: &mut Vec<UiRequest>) {
    let mut actions: Vec<Action> = node.param(key).unwrap_or_default();
    let mut changed = false;
    let mut remove = None;
    let mut swap = None;
    let count = actions.len();
    let steps = node.kind == ids::RATE;

    for (index, action) in actions.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            egui::Frame::new().fill(theme::bg_raised()).corner_radius(theme::RADIUS).inner_margin(7).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if steps {
                        ui.label(RichText::new(format!("{}.", index + 1)).color(theme::text_faint()));
                    }
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
                    let pick = SnipTarget::Action(node.id, key.to_owned(), index);
                    match action {
                        Action::Key { key, mode } => {
                            changed |= widgets::key_picker(ui, "key", key);
                            changed |= press_mode_picker(ui, mode);
                        }
                        Action::Mouse { button, mode, clicks, at } => {
                            changed |= widgets::mouse_button_picker(ui, "button", button);
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
                                *at = fixed.then(|| Point::on(0, 0, 0));
                                changed = true;
                            }
                            if let Some(point) = at {
                                changed |= widgets::point_edit(ui, point);
                                let mut display = point.display;
                                if widgets::display_picker(ui, "display", &mut display, displays, true) {
                                    *point = widgets::point_on(*point, display, displays);
                                    changed = true;
                                }
                                if ui.button(icon::CROSSHAIR).on_hover_text("Pick the position on screen").clicked() {
                                    requests.push(UiRequest::Snip(SnipMode::Point, pick));
                                }
                            }
                        }
                        Action::Move { x, y, display, relative, duration_ms } => {
                            let mut point = Point { x: *x, y: *y, display: *display };
                            if widgets::point_edit(ui, &mut point) {
                                (*x, *y) = (point.x, point.y);
                                changed = true;
                            }
                            if !*relative {
                                let mut chosen = *display;
                                if widgets::display_picker(ui, "display", &mut chosen, displays, true) {
                                    let moved = widgets::point_on(point, chosen, displays);
                                    (*x, *y, *display) = (moved.x, moved.y, moved.display);
                                    changed = true;
                                }
                                if ui.button(icon::CROSSHAIR).on_hover_text("Pick the position on screen").clicked() {
                                    requests.push(UiRequest::Snip(SnipMode::Point, pick));
                                }
                            }
                            ui.end_row();
                            changed |= ui
                                .checkbox(relative, "Relative")
                                .on_hover_text("Move by this much from the current position")
                                .changed();
                            let mut glide = *duration_ms as f64;
                            ui.label(RichText::new("over").color(theme::text_dim()));
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
    let (title, hint) = if steps {
        ("Add step", "Done after the others, one rate later")
    } else {
        ("Add input", "Done at the same time as the others")
    };
    ui.menu_button(format!("{}  {title}", icon::PLUS), |ui| {
        ui.label(RichText::new(hint).small().weak());
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
                ui.label(RichText::new(format!("{}.", index + 1)).color(theme::text_faint()));
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
        RichText::new("Checked top to bottom; the first true case is followed.").size(11.0).color(theme::text_faint()),
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
