//! Node definitions and the registry that holds them.
//!
//! A [`NodeDef`] is a *declarative description* of a node type: its ports, its
//! parameters and their editors, its documentation. It contains no behaviour.
//! Behaviour lives in the engines (`magi-engine` for Rust, `runtime/python`
//! for Python), which look nodes up by the same string id.
//!
//! Because definitions are plain data they can be serialized, which is what
//! lets the editor draw and edit nodes it has no special code for – the
//! foundation for add-ons contributing their own nodes.
//!
//! To add a built-in node: describe it in one of the submodules here, then
//! implement it in both engines. See `docs/ADDING_A_NODE.md`.

pub mod ai;
pub mod flow;
pub mod input;
pub mod logic;
pub mod notes;
pub mod script;
pub mod sound;
pub mod system;
pub mod variables;
pub mod vision;
pub mod window;

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::assets::AssetStore;
use crate::graph::Node;

/// Stable ids of the built-in node types.
pub mod ids {
    pub const START: &str = "flow.start";
    pub const DELAY: &str = "flow.delay";
    pub const REPEAT: &str = "flow.repeat";
    pub const DECISION: &str = "flow.decision";
    pub const WAIT_UNTIL: &str = "flow.wait_until";
    pub const STOP: &str = "flow.stop";
    pub const LOG: &str = "flow.log";
    pub const INPUT: &str = "input.action";
    pub const PLAY_RECORDING: &str = "input.recording";
    pub const FIND_IMAGE: &str = "vision.find_image";
    pub const FIND_COLOR: &str = "vision.find_color";
    pub const GATE: &str = "logic.gate";
    pub const CHANCE: &str = "logic.chance";
    pub const PYTHON: &str = "script.python";
    pub const AUTOCLICK: &str = "input.autoclick";
    pub const AUTOTYPE: &str = "input.autotype";
    pub const READ_TEXT: &str = "vision.read_text";
    pub const FOCUS_WINDOW: &str = "window.focus";
    pub const RATE: &str = "flow.rate";
    pub const DEFINE: &str = "flow.define";
    pub const CALL: &str = "flow.call";
    pub const VARIABLE: &str = "var.get";
    pub const SET_VARIABLE: &str = "var.set";
    pub const COUNTER: &str = "var.counter";
    pub const MATH: &str = "logic.math";
    pub const COMPARE: &str = "logic.compare";
    pub const TEXT: &str = "logic.text";
    pub const CLIPBOARD: &str = "system.clipboard";
    pub const NOTIFY: &str = "system.notify";
    pub const COMMENT: &str = "note.comment";
    pub const SOUND_LEVEL: &str = "sound.level";
    pub const SOUND_MATCH: &str = "sound.match";
    pub const AI_WATCH: &str = "ai.watch";
    pub const AI_CLASSIFY: &str = "ai.classify";
}

/// Ids of ports that engines and the editor refer to by name.
pub mod ports {
    /// The single flow input every flow node has.
    pub const FLOW_IN: &str = "in";
    /// The default flow output.
    pub const OUT: &str = "out";
    pub const LOOP: &str = "loop";
    pub const DONE: &str = "done";
    pub const ELSE: &str = "else";
    pub const TIMEOUT: &str = "timeout";

    /// Flow output of a Decision case.
    pub fn case(id: &str) -> String {
        format!("case:{id}")
    }

    /// Condition input of a Decision case.
    pub fn condition(id: &str) -> String {
        format!("cond:{id}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Flow,
    Input,
    Vision,
    Sound,
    Ai,
    Window,
    Variables,
    Logic,
    System,
    Script,
    Notes,
    Addon,
}

impl Category {
    pub const ALL: [Category; 12] = [
        Category::Flow,
        Category::Input,
        Category::Vision,
        Category::Sound,
        Category::Ai,
        Category::Window,
        Category::Variables,
        Category::Logic,
        Category::System,
        Category::Script,
        Category::Notes,
        Category::Addon,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Flow => "Flow",
            Category::Input => "Input",
            Category::Vision => "Vision",
            Category::Sound => "Sound",
            Category::Ai => "AI",
            Category::Window => "Window",
            Category::Variables => "Variables",
            Category::Logic => "Logic",
            Category::System => "System",
            Category::Script => "Script",
            Category::Notes => "Notes",
            Category::Addon => "Add-ons",
        }
    }
}

/// The type of value a data port carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataType {
    Bool,
    Number,
    Point,
    Text,
    #[default]
    Any,
}

impl DataType {
    pub fn label(self) -> &'static str {
        match self {
            DataType::Bool => "True / false",
            DataType::Number => "Number",
            DataType::Point => "Screen position",
            DataType::Text => "Text",
            DataType::Any => "Any value",
        }
    }

    /// Whether an output of type `self` may feed an input of type `input`.
    pub fn fits(self, input: DataType) -> bool {
        self == input || self == DataType::Any || input == DataType::Any
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortKind {
    /// Execution order: "after this, do that". Drawn top to bottom.
    Flow,
    /// A value. Drawn left to right.
    Data(DataType),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PortDef {
    pub id: String,
    pub label: String,
    /// Ignored for flow ports.
    #[serde(default)]
    pub ty: DataType,
}

/// A port on a concrete node, after dynamic ports have been resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Port {
    pub id: String,
    pub label: String,
    pub kind: PortKind,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodePorts {
    pub flow_in: bool,
    pub flow_out: Vec<Port>,
    pub data_in: Vec<Port>,
    pub data_out: Vec<Port>,
}

impl NodePorts {
    pub fn output(&self, id: &str) -> Option<&Port> {
        self.flow_out.iter().chain(&self.data_out).find(|p| p.id == id)
    }

    pub fn input(&self, id: &str) -> Option<Port> {
        if id == ports::FLOW_IN {
            return self.flow_in.then(|| Port {
                id: ports::FLOW_IN.to_owned(),
                label: String::new(),
                kind: PortKind::Flow,
            });
        }
        self.data_in.iter().find(|p| p.id == id).cloned()
    }

    /// True for nodes that take part in execution order (as opposed to pure value nodes).
    pub fn is_flow_node(&self) -> bool {
        self.flow_in || !self.flow_out.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChoiceOption {
    pub value: String,
    pub label: String,
}

/// How a parameter is stored and edited. The editor picks a widget per kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParamKind {
    Bool,
    Int {
        min: i64,
        max: i64,
        #[serde(default)]
        unit: String,
    },
    Float {
        min: f64,
        max: f64,
        #[serde(default)]
        unit: String,
    },
    /// Milliseconds.
    Duration,
    /// Stored as `0.0..=1.0`, edited as `0–100 %`.
    Percent,
    Text {
        #[serde(default)]
        multiline: bool,
    },
    Code {
        language: String,
    },
    Choice {
        options: Vec<ChoiceOption>,
    },
    Key,
    MouseButton,
    Hotkey,
    /// A screen position, or `null`.
    Point,
    /// A screen rectangle, or `null` for the whole screen.
    Region,
    /// `#RRGGBB`.
    Color,
    /// Id of an image asset.
    Image,
    /// Id of a recording asset.
    Recording,
    /// A list of [`Action`](crate::action::Action)s.
    Actions,
    /// A list of Decision [`Case`](flow::Case)s.
    Cases,
    /// Auto Clicker settings ([`ClickerSettings`](crate::autoclicker::ClickerSettings)).
    Clicker,
    /// The name of one of the project's variables.
    Variable,
    /// The name of a Definition block.
    Definition,
    /// The id of a block of the same macro, or `null`.
    Block,
    /// A display number (0 = primary).
    Display,
    /// Id of a sound asset.
    Audio,
    /// A list of images, each optionally in a named group with its own search
    /// area ([`vision::ImageTarget`]). The groups are the node's `groups` setting.
    Images,
    /// A list of sounds, each optionally in a named group
    /// ([`sound::SoundTarget`]). The groups are the node's `groups` setting.
    Sounds,
    /// The name of a sound input device; empty for the system's default.
    AudioDevice,
    /// Id of an AI model asset (a `.magic` file).
    Model,
    /// Values for the adjustable settings an AI model declares, by key.
    ModelSettings,
}

impl ParamKind {
    /// Whether settings of this kind can be bound to a variable.
    pub fn bindable(&self) -> bool {
        matches!(
            self,
            ParamKind::Bool
                | ParamKind::Int { .. }
                | ParamKind::Float { .. }
                | ParamKind::Duration
                | ParamKind::Percent
                | ParamKind::Text { .. }
                | ParamKind::Choice { .. }
                | ParamKind::Point
                | ParamKind::Region
                | ParamKind::Color
                | ParamKind::Display
        )
    }
}

impl ParamKind {
    pub fn choice(options: &[(&str, &str)]) -> Self {
        ParamKind::Choice {
            options: options
                .iter()
                .map(|(value, label)| ChoiceOption { value: (*value).into(), label: (*label).into() })
                .collect(),
        }
    }

    pub fn int(min: i64, max: i64, unit: &str) -> Self {
        ParamKind::Int { min, max, unit: unit.into() }
    }

    pub fn float(min: f64, max: f64, unit: &str) -> Self {
        ParamKind::Float { min, max, unit: unit.into() }
    }
}

/// Shows a parameter only while another parameter has a given value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShowIf {
    pub key: String,
    pub equals: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamDef {
    pub key: String,
    pub label: String,
    #[serde(flatten)]
    pub kind: ParamKind,
    #[serde(default)]
    pub default: Value,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub help: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_if: Option<ShowIf>,
    /// Kept for files and scripts, but not shown in the settings panel.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl ParamDef {
    pub fn new(key: &str, label: &str, kind: ParamKind) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            kind,
            default: Value::Null,
            help: String::new(),
            show_if: None,
            hidden: false,
        }
    }

    /// Keeps the setting out of the settings panel.
    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    pub fn help(mut self, text: &str) -> Self {
        self.help = text.into();
        self
    }

    pub fn show_if(mut self, key: &str, equals: impl Into<Value>) -> Self {
        self.show_if = Some(ShowIf { key: key.into(), equals: equals.into() });
        self
    }

    pub fn is_visible(&self, params: &Map<String, Value>) -> bool {
        !self.hidden && self.show_if.as_ref().is_none_or(|rule| params.get(&rule.key) == Some(&rule.equals))
    }
}

/// The asset ids a setting's value refers to.
fn asset_ids_in(kind: &ParamKind, value: &Value) -> Vec<String> {
    let listed = |field: &str| -> Vec<String> {
        value
            .as_array()
            .map(|items| items.iter().filter_map(|item| item.get(field)?.as_str()).map(str::to_owned).collect())
            .unwrap_or_default()
    };
    match kind {
        ParamKind::Image | ParamKind::Recording | ParamKind::Audio | ParamKind::Model => {
            value.as_str().filter(|id| !id.is_empty()).map(|id| vec![id.to_owned()]).unwrap_or_default()
        }
        ParamKind::Images => listed("image"),
        ParamKind::Sounds => listed("sound"),
        _ => Vec::new(),
    }
}

/// Replaces asset ids inside a setting's value.
fn rename_assets_in(kind: &ParamKind, value: &mut Value, rename: &dyn Fn(&str) -> Option<String>) {
    let mut in_list = |field: &str| {
        if let Some(items) = value.as_array_mut() {
            for item in items {
                if let Some(Value::String(id)) = item.get_mut(field)
                    && let Some(new) = rename(id)
                {
                    *id = new;
                }
            }
        }
    };
    match kind {
        ParamKind::Image | ParamKind::Recording | ParamKind::Audio | ParamKind::Model => {
            if let Value::String(id) = value
                && let Some(new) = rename(id)
            {
                *id = new;
            }
        }
        ParamKind::Images => in_list("image"),
        ParamKind::Sounds => in_list("sound"),
        _ => {}
    }
}

/// Ports that depend on a node's parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DynamicPorts {
    /// One condition input and one flow output per entry of a `Cases` parameter.
    Cases { param: String },
}

/// Which engines can run a node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineSupport {
    #[default]
    Any,
    PythonOnly,
}

/// Produces the lines of text shown on a node's face.
pub type SummaryFn = fn(&Node, &AssetStore) -> Vec<String>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeDef {
    /// Namespaced id, e.g. `flow.delay`. Written into project files.
    pub id: String,
    pub name: String,
    pub category: Category,
    pub description: String,
    /// Extra words the "add node" search should match.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub flow_in: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flow_out: Vec<PortDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_in: Vec<PortDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_out: Vec<PortDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ParamDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_ports: Option<DynamicPorts>,
    #[serde(default)]
    pub engines: EngineSupport,
    #[serde(skip)]
    pub summarize: Option<SummaryFn>,
}

impl NodeDef {
    pub fn new(id: &str, name: &str, category: Category, description: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            category,
            description: description.into(),
            keywords: Vec::new(),
            flow_in: false,
            flow_out: Vec::new(),
            data_in: Vec::new(),
            data_out: Vec::new(),
            params: Vec::new(),
            dynamic_ports: None,
            engines: EngineSupport::Any,
            summarize: None,
        }
    }

    pub fn keywords(mut self, words: &[&str]) -> Self {
        self.keywords = words.iter().map(|w| (*w).to_owned()).collect();
        self
    }

    pub fn flow_in(mut self) -> Self {
        self.flow_in = true;
        self
    }

    pub fn flow_out(mut self, id: &str, label: &str) -> Self {
        self.flow_out.push(PortDef { id: id.into(), label: label.into(), ty: DataType::Any });
        self
    }

    pub fn data_in(mut self, id: &str, label: &str, ty: DataType) -> Self {
        self.data_in.push(PortDef { id: id.into(), label: label.into(), ty });
        self
    }

    pub fn data_out(mut self, id: &str, label: &str, ty: DataType) -> Self {
        self.data_out.push(PortDef { id: id.into(), label: label.into(), ty });
        self
    }

    pub fn param(mut self, param: ParamDef) -> Self {
        self.params.push(param);
        self
    }

    /// Takes every parameter's default from a typed parameter struct, so the
    /// editor's defaults and the engine's defaults cannot drift apart.
    pub fn defaults<T: Serialize>(mut self, defaults: &T) -> Self {
        let Ok(Value::Object(map)) = serde_json::to_value(defaults) else {
            panic!("defaults for node `{}` must serialize to an object", self.id);
        };
        for param in &mut self.params {
            match map.get(&param.key) {
                Some(value) => param.default = value.clone(),
                None => panic!("node `{}`: no default for parameter `{}`", self.id, param.key),
            }
        }
        self
    }

    pub fn dynamic_ports(mut self, dynamic: DynamicPorts) -> Self {
        self.dynamic_ports = Some(dynamic);
        self
    }

    pub fn python_only(mut self) -> Self {
        self.engines = EngineSupport::PythonOnly;
        self
    }

    pub fn summarize(mut self, f: SummaryFn) -> Self {
        self.summarize = Some(f);
        self
    }

    pub fn default_params(&self) -> Map<String, Value> {
        self.params.iter().map(|p| (p.key.clone(), p.default.clone())).collect()
    }

    /// Every asset (image, recording, sound, model) a node of this type refers to.
    pub fn asset_ids(&self, node: &Node) -> Vec<String> {
        self.params
            .iter()
            .filter_map(|param| Some(asset_ids_in(&param.kind, node.params.get(&param.key)?)))
            .flatten()
            .collect()
    }

    /// Points a node at other assets: `rename` gives the new id for an old one.
    pub fn rename_assets(&self, node: &mut Node, rename: &dyn Fn(&str) -> Option<String>) {
        for param in &self.params {
            if let Some(value) = node.params.get_mut(&param.key) {
                rename_assets_in(&param.kind, value, rename);
            }
        }
    }

    /// Resolves the ports of a concrete node, including parameter-driven ones.
    pub fn ports(&self, node: &Node) -> NodePorts {
        let flow = |def: &PortDef| Port { id: def.id.clone(), label: def.label.clone(), kind: PortKind::Flow };
        let data = |def: &PortDef| Port { id: def.id.clone(), label: def.label.clone(), kind: PortKind::Data(def.ty) };
        let mut out = NodePorts {
            flow_in: self.flow_in,
            flow_out: Vec::new(),
            data_in: self.data_in.iter().map(data).collect(),
            data_out: self.data_out.iter().map(data).collect(),
        };
        if let Some(DynamicPorts::Cases { param }) = &self.dynamic_ports {
            for case in node.param::<Vec<flow::Case>>(param).unwrap_or_default() {
                out.data_in.push(Port {
                    id: ports::condition(&case.id),
                    label: case.label.clone(),
                    kind: PortKind::Data(DataType::Bool),
                });
                out.flow_out.push(Port { id: ports::case(&case.id), label: case.label, kind: PortKind::Flow });
            }
        }
        out.flow_out.extend(self.flow_out.iter().map(flow));
        out
    }
}

/// All node types known to this session.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    defs: BTreeMap<String, Arc<NodeDef>>,
}

impl Registry {
    pub fn empty() -> Self {
        Self::default()
    }

    /// The registry with every built-in node.
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        flow::register(&mut registry);
        input::register(&mut registry);
        vision::register(&mut registry);
        sound::register(&mut registry);
        ai::register(&mut registry);
        logic::register(&mut registry);
        script::register(&mut registry);
        window::register(&mut registry);
        flow::register_structure(&mut registry);
        variables::register(&mut registry);
        system::register(&mut registry);
        notes::register(&mut registry);
        registry
    }

    /// Adds a definition, replacing any existing one with the same id.
    pub fn register(&mut self, def: NodeDef) {
        self.defs.insert(def.id.clone(), Arc::new(def));
    }

    pub fn get(&self, id: &str) -> Option<&Arc<NodeDef>> {
        self.defs.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<NodeDef>> {
        self.defs.values()
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// Ports of a node. A node of an unknown type (e.g. from an add-on that is
    /// not installed) is shown as a plain pass-through step so the rest of the
    /// graph stays usable.
    pub fn ports(&self, node: &Node) -> NodePorts {
        match self.get(&node.kind) {
            Some(def) => def.ports(node),
            None => NodePorts {
                flow_in: true,
                flow_out: vec![Port { id: ports::OUT.into(), label: String::new(), kind: PortKind::Flow }],
                ..NodePorts::default()
            },
        }
    }

    /// The text shown on a node's face. Settings bound to variables are
    /// summarised with their defaults, and the variables are named.
    pub fn summary(&self, node: &Node, assets: &AssetStore) -> Vec<String> {
        self.summary_with(node, assets, &[])
    }

    /// The text shown on a node's face, with settings bound to variables
    /// summarised with the values the variables start with.
    pub fn summary_with(
        &self,
        node: &Node,
        assets: &AssetStore,
        variables: &[crate::variables::Variable],
    ) -> Vec<String> {
        let Some(def) = self.get(&node.kind) else {
            return vec![format!("Unknown node type “{}”", node.kind)];
        };
        let bound: Vec<(&String, &str)> = node
            .params
            .iter()
            .filter_map(|(key, value)| crate::variables::bound_name(value).map(|name| (key, name)))
            .collect();
        if bound.is_empty() {
            return def.summarize.map(|f| f(node, assets)).unwrap_or_default();
        }
        let mut shown = node.clone();
        let defaults = def.default_params();
        for (key, name) in &bound {
            let default = defaults.get(*key).cloned().unwrap_or(Value::Null);
            let value = match variables.iter().find(|v| v.name == *name) {
                Some(variable) => crate::variables::coerce(&variable.value, &default),
                None => default,
            };
            shown.params.insert((*key).clone(), value);
        }
        let mut lines = def.summarize.map(|f| f(&shown, assets)).unwrap_or_default();
        let mut names: Vec<&str> = bound.iter().map(|(_, name)| *name).collect();
        names.sort_unstable();
        names.dedup();
        lines.push(format!("Follows {}", names.join(", ")));
        lines
    }

    /// The title of a node: its custom label if set, else the type's name.
    pub fn title(&self, node: &Node) -> String {
        if !node.label.is_empty() {
            return node.label.clone();
        }
        self.get(&node.kind).map_or_else(|| node.kind.clone(), |def| def.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_is_well_formed() {
        let registry = Registry::builtin();
        assert!(registry.len() >= 14);
        for def in registry.iter() {
            assert!(def.id.contains('.'), "{} should be namespaced", def.id);
            assert!(!def.description.is_empty(), "{} needs a description", def.id);
            for param in &def.params {
                assert!(
                    !param.default.is_null()
                        || matches!(param.kind, ParamKind::Point | ParamKind::Region | ParamKind::Block),
                    "{}.{} has no default",
                    def.id,
                    param.key
                );
                if let Some(rule) = &param.show_if {
                    assert!(def.params.iter().any(|p| p.key == rule.key), "{}: bad show_if", def.id);
                }
            }
            let mut ids: Vec<&str> =
                def.flow_out.iter().chain(&def.data_in).chain(&def.data_out).map(|p| p.id.as_str()).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(
                ids.len(),
                def.flow_out.len() + def.data_in.len() + def.data_out.len(),
                "{} has duplicate port ids",
                def.id
            );
        }
    }

    #[test]
    fn definitions_are_plain_data() {
        let registry = Registry::builtin();
        let def = registry.get(ids::DECISION).unwrap();
        let json = serde_json::to_string(&**def).unwrap();
        let back: NodeDef = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, def.id);
        assert_eq!(back.params, def.params);
        assert_eq!(back.dynamic_ports, def.dynamic_ports);
    }
}
