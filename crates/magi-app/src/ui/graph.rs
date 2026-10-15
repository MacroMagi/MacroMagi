//! The node graph editor.
//!
//! Layout convention: **flow goes down, values go across**. A block's flow
//! input sits on its top edge and its flow outputs on its bottom edge, so a
//! macro reads like a flowchart from top to bottom. Value ports sit on the
//! left (inputs) and right (outputs) edges.
//!
//! Nodes are drawn by hand rather than built from egui widgets. That keeps
//! every node's geometry known up front – wires can be drawn under the nodes
//! in the same frame – and keeps the editor fast with hundreds of nodes.
//!
//! This module only edits the [`Project`] it is given. Anything that needs
//! the rest of the application (running a block, opening the inspector) is
//! reported as a [`GraphEvent`].

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Instant;

use egui::emath::TSTransform;
use egui::epaint::text::{LayoutJob, TextWrapping};
use egui::epaint::{CubicBezierShape, Shadow};
use egui::{
    Align2, Color32, CornerRadius, FontId, Galley, Id, Key, LayerId, Order, Painter, PointerButton, Pos2, Rect, Sense,
    Shape, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2,
};
use egui_phosphor::regular as icon;
use magi_core::action::Action;
use magi_core::graph::{Node, NodeId, PortRef, Wire};
use magi_core::nodes::flow::DecisionParams;
use magi_core::nodes::input::{ActionParams, PlayRecordingParams};
use magi_core::nodes::vision::FindImageParams;
use magi_core::nodes::{Category, NodeDef, Port, PortKind, Registry, ids, ports};
use magi_core::preset::NodePreset;
use magi_core::project::Project;
use magi_engine::Activity;

use super::images::{ImageCache, fit_size};
use super::recording_editor;
use crate::theme;

const NODE_WIDTH: f32 = 212.0;
const HEADER: f32 = 32.0;
const ROW: f32 = 22.0;
const LINE: f32 = 17.0;
const PAD: f32 = 10.0;
const FOOTER: f32 = 24.0;
const THUMBNAIL: f32 = 56.0;
const BUTTON_ROW: f32 = 24.0;
/// Minimum width a node reserves for each of its flow outputs.
const FLOW_SLOT: f32 = 86.0;
const PORT_RADIUS: f32 = 5.0;
/// Size of the invisible square around a port that reacts to the pointer.
const PORT_HIT: f32 = 20.0;
const NODE_RADIUS: u8 = 10;

const CARD_SIZE: Vec2 = vec2(360.0, 310.0);
const CARD_GAP: f32 = 34.0;

const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.25..=2.0;
/// How long a node keeps glowing after it ran, in seconds.
const GLOW_SECONDS: f32 = 0.6;
/// Prefix that marks clipboard text as copied MacroMagi nodes.
const CLIPBOARD_TAG: &str = "macromagi-nodes:";

pub enum GraphEvent {
    /// The run/stop button on a Start block was clicked.
    ToggleStart(NodeId),
    /// "Run from here" was chosen on a block.
    RunFrom(NodeId),
    /// A block was double-clicked: show its settings.
    Inspect(NodeId),
    /// Open a recording in the pop-out editor.
    EditRecording(String),
    Toast(String),
}

/// What the editor needs to know about the outside world to draw a frame.
pub struct GraphEnv<'a> {
    pub registry: &'a Registry,
    pub activity: &'a HashMap<NodeId, Activity>,
    /// Start blocks with an active run.
    pub running: &'a [NodeId],
    /// The block the last error was reported against.
    pub error_node: Option<NodeId>,
}

/// A wire being dragged out of a port.
#[derive(Clone)]
struct WireDrag {
    port: PortRef,
    /// Whether the drag started at an output (and is looking for an input).
    from_output: bool,
    kind: PortKind,
}

/// The "add block" popup.
struct Palette {
    /// Where the new block goes, in scene coordinates.
    scene_pos: Pos2,
    /// Where the popup is shown, in screen coordinates.
    screen_pos: Pos2,
    query: String,
    /// A dangling wire to attach to the new block.
    wire: Option<WireDrag>,
    /// Set on the frame the list opens. The click that opened it must not
    /// count as a click elsewhere, which would close it again at once.
    just_opened: bool,
    /// The row chosen with the arrow keys.
    highlight: usize,
}

pub struct GraphView {
    /// Screen position of the scene origin, relative to the canvas corner.
    pan: Vec2,
    zoom: f32,
    pub selection: BTreeSet<NodeId>,
    /// Draw order, back to front.
    order: Vec<NodeId>,
    wire_drag: Option<WireDrag>,
    /// Corner where a rubber-band selection began, in scene coordinates.
    band_start: Option<Pos2>,
    /// Scene position of the last right-click, for "paste here".
    menu_pos: Pos2,
    menu_wire: Option<Wire>,
    palette: Option<Palette>,
    /// Recording blocks whose information card is open.
    cards: BTreeSet<NodeId>,
    /// Card rectangles of the last frame (scene coordinates): scrolling over
    /// one scrolls its list instead of zooming the canvas.
    card_rects: Vec<Rect>,
    fit_requested: bool,
    /// Where the canvas was on screen last frame.
    canvas: Rect,
    pub images: ImageCache,
    /// Size of the block list; the app keeps it in the settings.
    pub palette_size: Vec2,
}

impl Default for GraphView {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
            selection: BTreeSet::new(),
            order: Vec::new(),
            wire_drag: None,
            band_start: None,
            menu_pos: Pos2::ZERO,
            menu_wire: None,
            palette: None,
            cards: BTreeSet::new(),
            card_rects: Vec::new(),
            fit_requested: true,
            canvas: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
            images: ImageCache::default(),
            palette_size: vec2(280.0, 420.0),
        }
    }
}

/// A port's place on screen.
struct Spot {
    port: Port,
    pos: Pos2,
    is_input: bool,
}

/// Where everything of one node goes, in scene coordinates.
struct Layout {
    id: NodeId,
    rect: Rect,
    def: Option<Arc<NodeDef>>,
    title: String,
    spots: Vec<Spot>,
    rows_top: f32,
    lines: Vec<String>,
    lines_top: f32,
    thumbnail: Option<(Rect, String)>,
    button: Option<(Rect, &'static str)>,
}

impl Layout {
    fn compute(node: &Node, project: &Project, registry: &Registry) -> Self {
        let node_ports = registry.ports(node);
        let lines = registry.summary(node, &project.assets);
        let width = NODE_WIDTH.max(node_ports.flow_out.len() as f32 * FLOW_SLOT);
        let origin = pos2(node.pos[0], node.pos[1]);

        let rows = node_ports.data_in.len().max(node_ports.data_out.len());
        let rows_top = origin.y + HEADER + if rows > 0 { 6.0 } else { 0.0 };
        let mut y = rows_top + rows as f32 * ROW;
        let lines_top = y + if lines.is_empty() { 0.0 } else { 6.0 };
        y = lines_top + lines.len() as f32 * LINE;

        let mut thumbnail = None;
        if node.kind == ids::FIND_IMAGE {
            let image = node.params_or_default::<FindImageParams>().image;
            if project.assets.image(&image).is_some() {
                thumbnail = Some((
                    Rect::from_min_size(pos2(origin.x + PAD, y + 6.0), vec2(width - 2.0 * PAD, THUMBNAIL)),
                    image,
                ));
                y += THUMBNAIL + 6.0;
            }
        }

        let label = match node.kind.as_str() {
            ids::INPUT => Some("Add input"),
            ids::DECISION => Some("Add case"),
            _ => None,
        };
        let button = label.map(|label| {
            let rect = Rect::from_min_size(pos2(origin.x + PAD, y + 8.0), vec2(width - 2.0 * PAD, BUTTON_ROW));
            y += BUTTON_ROW + 8.0;
            (rect, label)
        });

        let labelled_outputs = node_ports.flow_out.iter().any(|port| !port.label.is_empty());
        y += if labelled_outputs { FOOTER } else { PAD };
        let rect = Rect::from_min_max(origin, pos2(origin.x + width, y));

        let mut spots = Vec::new();
        if node_ports.flow_in {
            let port = Port { id: ports::FLOW_IN.into(), label: String::new(), kind: PortKind::Flow };
            spots.push(Spot { port, pos: rect.center_top(), is_input: true });
        }
        let outputs = node_ports.flow_out.len() as f32;
        for (i, port) in node_ports.flow_out.iter().enumerate() {
            let x = rect.left() + width * (i as f32 + 0.5) / outputs;
            spots.push(Spot { port: port.clone(), pos: pos2(x, rect.bottom()), is_input: false });
        }
        let row_y = |i: usize| rows_top + i as f32 * ROW + ROW / 2.0;
        for (i, port) in node_ports.data_in.iter().enumerate() {
            spots.push(Spot { port: port.clone(), pos: pos2(rect.left(), row_y(i)), is_input: true });
        }
        for (i, port) in node_ports.data_out.iter().enumerate() {
            spots.push(Spot { port: port.clone(), pos: pos2(rect.right(), row_y(i)), is_input: false });
        }

        Self {
            id: node.id,
            rect,
            def: registry.get(&node.kind).cloned(),
            title: registry.title(node),
            spots,
            rows_top,
            lines,
            lines_top,
            thumbnail,
            button,
        }
    }

    fn header(&self) -> Rect {
        Rect::from_min_size(self.rect.min, vec2(self.rect.width(), HEADER))
    }

    /// The small button at the right end of the header.
    fn header_button(&self) -> Rect {
        Rect::from_center_size(pos2(self.rect.right() - 18.0, self.rect.top() + HEADER / 2.0), Vec2::splat(22.0))
    }

    fn spot(&self, port: &str, is_input: bool) -> Option<&Spot> {
        self.spots.iter().find(|spot| spot.is_input == is_input && spot.port.id == port)
    }

    fn category(&self) -> Category {
        self.def.as_ref().map_or(Category::Addon, |def| def.category)
    }
}

/// The icon shown in a node's header and in the palette.
pub fn node_icon(kind: &str) -> &'static str {
    match kind {
        ids::START => icon::FLAG,
        ids::DELAY => icon::TIMER,
        ids::REPEAT => icon::REPEAT,
        ids::DECISION => icon::ARROWS_SPLIT,
        ids::WAIT_UNTIL => icon::HOURGLASS,
        ids::STOP => icon::PROHIBIT,
        ids::LOG => icon::CHAT_TEXT,
        ids::INPUT => icon::CURSOR_CLICK,
        ids::PLAY_RECORDING => icon::RECORD,
        ids::FIND_IMAGE => icon::IMAGE,
        ids::FIND_COLOR => icon::EYEDROPPER,
        ids::GATE => icon::TOGGLE_RIGHT,
        ids::CHANCE => icon::DICE_FIVE,
        ids::PYTHON => icon::CODE,
        ids::AUTOCLICK => icon::HAND_TAP,
        ids::FOCUS_WINDOW => icon::APP_WINDOW,
        _ => icon::SPARKLE,
    }
}

fn fit_text(painter: &Painter, text: &str, font: FontId, color: Color32, max_width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = TextWrapping::truncate_at_width(max_width.max(8.0));
    painter.layout_job(job)
}

/// The curve of a wire between two ports. Flow wires leave and arrive
/// vertically, value wires horizontally.
fn wire_points(from: Pos2, to: Pos2, kind: PortKind) -> [Pos2; 4] {
    match kind {
        PortKind::Flow => {
            let reach = ((to.y - from.y).abs() * 0.5).clamp(36.0, 160.0);
            [from, pos2(from.x, from.y + reach), pos2(to.x, to.y - reach), to]
        }
        PortKind::Data(_) => {
            let reach = ((to.x - from.x).abs() * 0.5).clamp(36.0, 160.0);
            [from, pos2(from.x + reach, from.y), pos2(to.x - reach, to.y), to]
        }
    }
}

fn bezier(points: [Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    let weights = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    let mut out = Vec2::ZERO;
    for (point, weight) in points.iter().zip(weights) {
        out += point.to_vec2() * weight;
    }
    out.to_pos2()
}

fn distance_to_wire(points: [Pos2; 4], pos: Pos2) -> f32 {
    (0..=24).map(|i| bezier(points, i as f32 / 24.0).distance(pos)).fold(f32::INFINITY, f32::min)
}

fn paint_wire(painter: &Painter, points: [Pos2; 4], color: Color32, width: f32) {
    painter.add(CubicBezierShape::from_points_stroke(points, false, Color32::TRANSPARENT, Stroke::new(width, color)));
}

fn paint_port(painter: &Painter, spot: &Spot, connected: bool, emphasis: f32) {
    let color = theme::port_color(spot.port.kind);
    let fill = if connected { color } else { theme::bg_canvas() };
    let stroke = Stroke::new(1.6, color);
    let grow = 1.0 + 0.35 * emphasis;
    match spot.port.kind {
        PortKind::Flow => {
            // A downward chevron: "execution continues this way".
            let (w, h) = (7.0 * grow, 6.0 * grow);
            let c = spot.pos;
            let points = vec![pos2(c.x - w, c.y - h / 2.0), pos2(c.x + w, c.y - h / 2.0), pos2(c.x, c.y + h)];
            painter.add(Shape::convex_polygon(points, fill, stroke));
        }
        PortKind::Data(_) => {
            painter.circle(spot.pos, PORT_RADIUS * grow, fill, stroke);
        }
    }
}

impl GraphView {
    fn to_screen(&self, canvas: Rect) -> TSTransform {
        TSTransform::new(canvas.min.to_vec2() + self.pan, self.zoom)
    }

    /// Scene position at the centre of the visible canvas.
    pub fn view_center(&self) -> Pos2 {
        self.to_screen(self.canvas).inverse() * self.canvas.center()
    }

    /// A scene position for a new block: under the lowest block if that is in
    /// view, otherwise the middle of the view.
    pub fn free_spot(&self, project: &Project) -> Pos2 {
        let center = self.view_center();
        let visible = self.to_screen(self.canvas).inverse() * self.canvas;
        let lowest = project.graph.nodes().max_by(|a, b| a.pos[1].total_cmp(&b.pos[1]));
        match lowest {
            Some(node) if visible.contains(pos2(node.pos[0], node.pos[1])) => pos2(node.pos[0], node.pos[1] + 170.0),
            _ => center - vec2(NODE_WIDTH / 2.0, 40.0),
        }
    }

    /// Opens the information card of a recording block.
    pub fn show_card(&mut self, node: NodeId) {
        self.cards.insert(node);
    }

    /// The layer the nodes and wires are drawn on.
    fn scene_layer(ui: &Ui) -> LayerId {
        LayerId::new(ui.layer_id().order, ui.id().with("graph-scene"))
    }

    /// Whether the pointer is over the canvas itself or a block on it – and
    /// not over something floating above (a window, a menu, the block list).
    fn pointer_on_canvas(ui: &Ui, canvas: Rect) -> bool {
        let Some(pointer) = ui.ctx().pointer_latest_pos().filter(|p| canvas.contains(*p)) else {
            return false;
        };
        // Blocks live on the scene layer, so `Ui::rect_contains_pointer` alone
        // would say "no" whenever the pointer is over one.
        ui.ctx().layer_id_at(pointer).is_none_or(|top| top == ui.layer_id() || top == Self::scene_layer(ui))
    }

    fn zoom_about(&mut self, canvas: Rect, screen_pos: Pos2, factor: f32) {
        let before = self.to_screen(canvas).inverse() * screen_pos;
        self.zoom = (self.zoom * factor).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
        // Keep the scene point under the pointer where it was.
        self.pan = screen_pos.to_vec2() - canvas.min.to_vec2() - before.to_vec2() * self.zoom;
    }

    fn fit(&mut self, canvas: Rect, layouts: &[Layout]) {
        let Some(bounds) = layouts.iter().map(|l| l.rect).reduce(|a, b| a.union(b)) else {
            self.zoom = 1.0;
            self.pan = canvas.size() / 2.0 - vec2(NODE_WIDTH / 2.0, 60.0);
            return;
        };
        let margin = 70.0;
        let scale = (canvas.width() / (bounds.width() + margin)).min(canvas.height() / (bounds.height() + margin));
        self.zoom = scale.clamp(*ZOOM_RANGE.start(), 1.0);
        self.pan = canvas.size() / 2.0 - bounds.center().to_vec2() * self.zoom;
    }

    /// Makes one block the selection and scrolls it into view.
    pub fn focus_node(&mut self, node: NodeId) {
        self.selection = BTreeSet::from([node]);
        self.bring_to_front(node);
    }

    fn bring_to_front(&mut self, node: NodeId) {
        self.order.retain(|id| *id != node);
        self.order.push(node);
    }

    /// Adds a block of the given type and selects it.
    pub fn add_node(&mut self, project: &mut Project, registry: &Registry, kind: &str, at: Pos2) -> Option<NodeId> {
        let id = project.graph.add(registry, kind, [at.x.round(), at.y.round()]).ok()?;
        self.focus_node(id);
        Some(id)
    }

    pub fn delete_selection(&mut self, project: &mut Project) {
        for id in std::mem::take(&mut self.selection) {
            project.graph.remove(id);
        }
    }

    pub fn copy_selection(&self, ui: &Ui, project: &Project, registry: &Registry) {
        if self.selection.is_empty() {
            return;
        }
        // Copied as a self-contained node preset, so pasting into another
        // project (or another MacroMagi window) brings images and recordings along.
        let preset = NodePreset::capture(&project.graph, &self.selection, &project.assets, registry);
        if let Ok(json) = serde_json::to_string(&preset) {
            ui.ctx().copy_text(format!("{CLIPBOARD_TAG}{json}"));
        }
    }

    pub fn paste(&mut self, project: &mut Project, registry: &Registry, text: &str, at: Pos2) -> bool {
        let Some(json) = text.strip_prefix(CLIPBOARD_TAG) else { return false };
        let Ok(preset) = serde_json::from_str::<NodePreset>(json) else { return false };
        self.insert_preset(project, registry, &preset, at)
    }

    /// Adds a group of pre-configured blocks and selects them.
    pub fn insert_preset(&mut self, project: &mut Project, registry: &Registry, preset: &NodePreset, at: Pos2) -> bool {
        match preset.insert_into(&mut project.graph, &mut project.assets, registry, [at.x.round(), at.y.round()]) {
            Ok(ids) => {
                self.selection = ids.iter().copied().collect();
                for id in ids {
                    self.bring_to_front(id);
                }
                true
            }
            Err(_) => false,
        }
    }

    fn duplicate_selection(&mut self, project: &mut Project) {
        if self.selection.is_empty() {
            return;
        }
        let fragment = project.graph.fragment(&self.selection);
        let origin = fragment.origin();
        let ids = project.graph.insert_fragment(&fragment, [origin[0] + 28.0, origin[1] + 28.0]);
        self.selection = ids.iter().copied().collect();
        for id in ids {
            self.bring_to_front(id);
        }
    }

    /// Opens the "add block" popup at a screen position.
    pub fn open_palette(&mut self, screen_pos: Pos2) {
        self.palette = Some(Palette {
            scene_pos: self.to_screen(self.canvas).inverse() * screen_pos,
            screen_pos,
            query: String::new(),
            wire: None,
            just_opened: true,
            highlight: 0,
        });
    }

    /// Opens the block list near the top-left of the canvas; the new block
    /// goes in the middle of the view.
    pub fn open_palette_in_view(&mut self) {
        self.open_palette(self.canvas.min + vec2(16.0, 16.0));
        let center = self.view_center() - vec2(0.0, 40.0);
        if let Some(palette) = &mut self.palette {
            palette.scene_pos = center;
        }
    }

    #[cfg(test)]
    pub fn palette_is_open(&self) -> bool {
        self.palette.is_some()
    }

    pub fn request_fit(&mut self) {
        self.fit_requested = true;
    }

    pub fn reset_zoom(&mut self) {
        let factor = 1.0 / self.zoom;
        self.zoom_about(self.canvas, self.canvas.center(), factor);
    }

    /// Draws the editor into the remaining space of `ui`.
    pub fn show(&mut self, ui: &mut Ui, project: &mut Project, env: &GraphEnv<'_>) -> Vec<GraphEvent> {
        let mut events = Vec::new();
        let (canvas, background) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.canvas = canvas;
        ui.painter().rect_filled(canvas, 0.0, theme::bg_canvas());

        // Keep the draw order in step with the graph.
        self.order.retain(|id| project.graph.contains(*id));
        for node in project.graph.nodes() {
            if !self.order.contains(&node.id) {
                self.order.push(node.id);
            }
        }
        self.selection.retain(|id| project.graph.contains(*id));
        self.cards.retain(|id| project.graph.contains(*id));
        self.images.retain_existing(&project.assets);

        let mut layouts: Vec<Layout> = self
            .order
            .iter()
            .filter_map(|id| project.graph.node(*id))
            .map(|node| Layout::compute(node, project, env.registry))
            .collect();
        if std::mem::take(&mut self.fit_requested) {
            self.fit(canvas, &layouts);
        }

        self.navigate(ui, canvas, &background);
        self.paint_grid(ui.painter(), canvas);

        // The scene: a layer whose contents are panned and zoomed as a whole.
        let to_screen = self.to_screen(canvas);
        let from_screen = to_screen.inverse();
        let layer = Self::scene_layer(ui);
        ui.ctx().set_sublayer(ui.layer_id(), layer);
        let mut scene = ui.new_child(
            UiBuilder::new()
                .layer_id(layer)
                .max_rect(Rect::from_center_size(Pos2::ZERO, Vec2::splat(2.0e6)))
                .sense(Sense::hover()),
        );
        scene.set_clip_rect(from_screen * canvas);
        ui.ctx().set_transform_layer(layer, to_screen);
        let painter = scene.painter().clone();
        let pointer = ui.ctx().pointer_latest_pos().filter(|p| canvas.contains(*p)).map(|p| from_screen * p);

        // ---- wires (under the nodes) --------------------------------------
        let by_id: HashMap<NodeId, usize> = layouts.iter().enumerate().map(|(i, l)| (l.id, i)).collect();
        let mut hovered_wire = None;
        for wire in project.graph.wires() {
            let (Some(&a), Some(&b)) = (by_id.get(&wire.from.node), by_id.get(&wire.to.node)) else { continue };
            let (Some(from), Some(to)) =
                (layouts[a].spot(&wire.from.port, false), layouts[b].spot(&wire.to.port, true))
            else {
                continue;
            };
            let points = wire_points(from.pos, to.pos, from.port.kind);
            let near = pointer.is_some_and(|p| distance_to_wire(points, p) < 7.0 / self.zoom);
            if near && self.wire_drag.is_none() {
                hovered_wire = Some(wire.clone());
            }
            let color = theme::port_color(from.port.kind);
            let width = if matches!(from.port.kind, PortKind::Flow) { 2.4 } else { 2.0 };
            if near && self.wire_drag.is_none() {
                paint_wire(&painter, points, theme::with_alpha(color, 0.25), width + 5.0);
            }
            paint_wire(&painter, points, color, width);
        }

        // ---- nodes ----------------------------------------------------------
        let now = Instant::now();
        let mut glowing = false;
        let mut drag_delta = Vec2::ZERO;
        let mut hovered_port: Option<(PortRef, bool, PortKind, Pos2)> = None;
        let mut card_rects = Vec::new();

        for layout in &layouts {
            let Some(node) = project.graph.node(layout.id).cloned() else { continue };
            let id = scene.id().with(("node", layout.id));

            // The body is registered first so its ports and buttons end up on top of it.
            let body = scene.interact(layout.rect, id, Sense::click_and_drag());
            let modifiers = ui.input(|i| i.modifiers);
            if body.clicked() {
                if modifiers.shift || modifiers.command {
                    if !self.selection.remove(&layout.id) {
                        self.selection.insert(layout.id);
                    }
                } else {
                    self.selection = BTreeSet::from([layout.id]);
                }
            }
            if body.double_clicked() {
                events.push(GraphEvent::Inspect(layout.id));
            }
            if body.drag_started_by(PointerButton::Primary) && !self.selection.contains(&layout.id) {
                if !(modifiers.shift || modifiers.command) {
                    self.selection.clear();
                }
                self.selection.insert(layout.id);
            }
            if body.dragged_by(PointerButton::Primary) {
                drag_delta += body.drag_delta();
            }
            if body.dragged_by(PointerButton::Middle) || body.dragged_by(PointerButton::Secondary) {
                self.pan += body.drag_delta() * self.zoom;
            }
            body.context_menu(|ui| self.node_menu(ui, project, env.registry, &node, &mut events));

            let glow = env
                .activity
                .get(&layout.id)
                .map_or(0.0, |a| 1.0 - now.saturating_duration_since(a.last).as_secs_f32() / GLOW_SECONDS)
                .clamp(0.0, 1.0);
            glowing |= glow > 0.0;
            let running = env.running.contains(&layout.id);
            self.paint_node(
                &painter,
                layout,
                &node,
                NodeLook {
                    selected: self.selection.contains(&layout.id),
                    hovered: body.hovered(),
                    glow,
                    running,
                    error: env.error_node == Some(layout.id),
                },
            );
            if let Some((rect, asset)) = &layout.thumbnail
                && let Some(texture) = self.images.get(ui.ctx(), &project.assets, asset)
            {
                painter.rect_filled(*rect, 5.0, theme::bg_input());
                let size = fit_size(&texture, rect.size() - Vec2::splat(6.0), 4.0);
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                let tint = if node.enabled { Color32::WHITE } else { Color32::from_white_alpha(90) };
                painter.image(texture.id(), Rect::from_center_size(rect.center(), size), uv, tint);
            }

            // Header button: run/stop on Start blocks, the info card on recordings.
            let header_action = match node.kind.as_str() {
                ids::START => Some(if running { icon::STOP } else { icon::PLAY }),
                ids::PLAY_RECORDING => {
                    Some(if self.cards.contains(&layout.id) { icon::CARET_LEFT } else { icon::LIST })
                }
                _ => None,
            };
            if let Some(glyph) = header_action {
                let rect = layout.header_button();
                let response = scene.interact(rect, id.with("header-button"), Sense::click());
                let color = match (node.kind.as_str(), running) {
                    (ids::START, true) => theme::danger(),
                    (ids::START, false) => theme::success(),
                    _ => theme::text_dim(),
                };
                if response.hovered() {
                    painter.rect_filled(rect, 5.0, theme::with_alpha(color, 0.18));
                }
                painter.text(rect.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(14.0), color);
                if response.clicked() {
                    if node.kind == ids::START {
                        events.push(GraphEvent::ToggleStart(layout.id));
                    } else if !self.cards.remove(&layout.id) {
                        self.cards.insert(layout.id);
                    }
                }
                let hint = match (node.kind.as_str(), running) {
                    (ids::START, true) => "Stop this Start block",
                    (ids::START, false) => "Run from this Start block",
                    _ => "Show the recorded inputs",
                };
                response.on_hover_text(hint);
            }

            // "Add input" / "Add case".
            if let Some((rect, label)) = layout.button {
                let response = scene.interact(rect, id.with("add"), Sense::click());
                let fill = if response.hovered() { theme::bg_widget_hover() } else { theme::bg_widget() };
                painter.rect_filled(rect, 5.0, fill);
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    format!("{}  {label}", icon::PLUS),
                    FontId::proportional(11.5),
                    theme::text_dim(),
                );
                if node.kind == ids::DECISION {
                    if response.clicked()
                        && let Some(target) = project.graph.node_mut(layout.id)
                    {
                        let mut params: DecisionParams = target.params_or_default();
                        params.add_case();
                        target.set_params(&params);
                    }
                } else {
                    egui::Popup::menu(&response).at_pointer_fixed().show(|ui| {
                        ui.label(egui::RichText::new("Add an input to do at the same time").small().weak());
                        for kind in Action::KINDS {
                            if ui.button(Action::kind_label(kind)).clicked() {
                                if let (Some(target), Some(action)) =
                                    (project.graph.node_mut(layout.id), Action::default_of(kind))
                                {
                                    let mut params: ActionParams = target.params_or_default();
                                    params.actions.push(action);
                                    target.set_params(&params);
                                }
                                self.selection = BTreeSet::from([layout.id]);
                                ui.close();
                            }
                        }
                    });
                }
            }

            // Ports.
            for spot in &layout.spots {
                let port_ref = PortRef::new(layout.id, spot.port.id.clone());
                let hit = Rect::from_center_size(spot.pos, Vec2::splat(PORT_HIT));
                let response =
                    scene.interact(hit, id.with(("port", &spot.port.id, spot.is_input)), Sense::click_and_drag());
                let pointer_over = pointer.is_some_and(|p| hit.contains(p));
                if pointer_over {
                    hovered_port = Some((port_ref.clone(), spot.is_input, spot.port.kind, spot.pos));
                }

                if response.drag_started_by(PointerButton::Primary) {
                    // Dragging off a wired value input picks the wire up by its other end.
                    let existing = (spot.is_input && matches!(spot.port.kind, PortKind::Data(_)))
                        .then(|| project.graph.data_source(layout.id, &spot.port.id).cloned())
                        .flatten();
                    self.wire_drag = Some(match existing {
                        Some(source) => {
                            project.graph.disconnect_port(&port_ref);
                            WireDrag { port: source, from_output: true, kind: spot.port.kind }
                        }
                        None => WireDrag { port: port_ref.clone(), from_output: !spot.is_input, kind: spot.port.kind },
                    });
                }
                if response.secondary_clicked() {
                    project.graph.disconnect_port(&port_ref);
                }

                let connected = project.graph.is_connected(&port_ref);
                let compatible = self.wire_drag.as_ref().is_some_and(|drag| {
                    drag.from_output == spot.is_input && self.would_connect(project, env.registry, drag, &port_ref)
                });
                let emphasis = if (pointer_over && self.wire_drag.is_none()) || (compatible && pointer_over) {
                    1.0
                } else if compatible {
                    0.5
                } else {
                    0.0
                };
                paint_port(&painter, spot, connected, emphasis);
                if self.wire_drag.is_none() {
                    let what = match spot.port.kind {
                        PortKind::Flow if spot.is_input => "Flow in – runs after whatever is wired here".to_owned(),
                        PortKind::Flow => "Flow out – what runs next".to_owned(),
                        PortKind::Data(ty) => format!("{} ({})", spot.port.label, ty.label()),
                    };
                    response.on_hover_text(what);
                }
            }

            // The recording information card, tethered to the block's right side.
            if node.kind == ids::PLAY_RECORDING && self.cards.contains(&layout.id) {
                let rect = Rect::from_min_size(pos2(layout.rect.right() + CARD_GAP, layout.rect.top()), CARD_SIZE);
                card_rects.push(rect);
                self.recording_card(&mut scene, &painter, project, layout, &node, rect, &mut events);
            }
        }
        self.card_rects = card_rects;

        // ---- moving the selection -------------------------------------------
        if drag_delta != Vec2::ZERO {
            for id in &self.selection {
                if let Some(node) = project.graph.node_mut(*id) {
                    node.pos = [node.pos[0] + drag_delta.x, node.pos[1] + drag_delta.y];
                }
            }
        }
        if let Some(top) = self.selection.iter().next_back().copied()
            && ui.input(|i| i.pointer.any_pressed())
            && layouts.iter().any(|l| l.id == top && pointer.is_some_and(|p| l.rect.contains(p)))
        {
            self.bring_to_front(top);
        }

        // ---- the wire being dragged -----------------------------------------
        if let Some(drag) = self.wire_drag.clone() {
            let start = by_id
                .get(&drag.port.node)
                .and_then(|&i| layouts[i].spot(&drag.port.port, !drag.from_output))
                .map(|spot| spot.pos);
            let target = hovered_port.as_ref().filter(|(port, is_input, ..)| {
                *is_input == drag.from_output && self.would_connect(project, env.registry, &drag, port)
            });
            let end = target.map(|(.., pos)| *pos).or(pointer);
            if let (Some(start), Some(end)) = (start, end) {
                let (from, to) = if drag.from_output { (start, end) } else { (end, start) };
                let color = theme::port_color(drag.kind);
                paint_wire(&painter, wire_points(from, to, drag.kind), theme::with_alpha(color, 0.85), 2.4);
            }
            if ui.input(|i| i.pointer.any_released()) {
                self.wire_drag = None;
                match (target, pointer) {
                    (Some((port, ..)), _) => {
                        let (from, to) = if drag.from_output {
                            (drag.port.clone(), port.clone())
                        } else {
                            (port.clone(), drag.port.clone())
                        };
                        let _ = project.graph.connect(env.registry, from, to);
                    }
                    // Dropped on empty canvas: offer to add a block there, already wired up.
                    (None, Some(scene_pos))
                        if hovered_port.is_none() && !layouts.iter().any(|l| l.rect.contains(scene_pos)) =>
                    {
                        self.palette = Some(Palette {
                            scene_pos,
                            screen_pos: to_screen * scene_pos,
                            query: String::new(),
                            wire: Some(drag),
                            just_opened: true,
                            highlight: 0,
                        });
                    }
                    _ => {}
                }
            }
        }

        // ---- background: selection band, wire cutting, menu ---------------
        self.background(ui, &painter, project, env, &background, canvas, &layouts, hovered_wire);
        self.shortcuts(ui, project, env.registry, canvas, &background);
        self.palette_popup(ui, project, env.registry);
        self.overlay(ui, canvas, project.graph.is_empty());

        // Re-lay-out after edits made this frame is not needed: the next frame does it.
        layouts.clear();
        if glowing || !env.running.is_empty() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
        }
        events
    }

    fn would_connect(&self, project: &Project, registry: &Registry, drag: &WireDrag, other: &PortRef) -> bool {
        let (from, to) = if drag.from_output { (&drag.port, other) } else { (other, &drag.port) };
        project.graph.can_connect(registry, from, to).is_ok()
    }

    /// Panning and zooming.
    fn navigate(&mut self, ui: &Ui, canvas: Rect, background: &egui::Response) {
        let space_held = ui.input(|i| i.key_down(Key::Space)) && !ui.ctx().text_edit_focused();
        if background.dragged_by(PointerButton::Middle)
            || background.dragged_by(PointerButton::Secondary)
            || (space_held && background.dragged_by(PointerButton::Primary))
        {
            self.pan += background.drag_delta();
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }

        let Some(pointer) = ui.ctx().pointer_latest_pos().filter(|p| canvas.contains(*p)) else { return };
        if !Self::pointer_on_canvas(ui, canvas) {
            return;
        }
        let over_card = {
            let scene_pos = self.to_screen(canvas).inverse() * pointer;
            self.card_rects.iter().any(|rect| rect.contains(scene_pos))
        };
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta(), i.zoom_delta()));
        if pinch != 1.0 {
            self.zoom_about(canvas, pointer, pinch);
        }
        if scroll.y != 0.0 && !over_card {
            self.zoom_about(canvas, pointer, (scroll.y * 0.0022).exp());
        }
    }

    fn paint_grid(&self, painter: &Painter, canvas: Rect) {
        let painter = painter.with_clip_rect(canvas);
        // Double the spacing while zoomed out, so lines never crowd together.
        let mut step = 24.0 * self.zoom;
        while step < 16.0 {
            step *= 2.0;
        }
        let origin = canvas.min + self.pan;
        let minor = Stroke::new(1.0, theme::mix(theme::bg_canvas(), theme::grid(), 0.6));
        let major = Stroke::new(1.0, theme::mix(theme::grid(), theme::stroke(), 0.45));
        let first_x = ((canvas.left() - origin.x) / step).floor() as i32;
        let first_y = ((canvas.top() - origin.y) / step).floor() as i32;
        let mut index = first_x;
        loop {
            let x = origin.x + index as f32 * step;
            if x > canvas.right() {
                break;
            }
            painter.vline(x, canvas.y_range(), if index % 5 == 0 { major } else { minor });
            index += 1;
        }
        index = first_y;
        loop {
            let y = origin.y + index as f32 * step;
            if y > canvas.bottom() {
                break;
            }
            painter.hline(canvas.x_range(), y, if index % 5 == 0 { major } else { minor });
            index += 1;
        }
    }

    fn paint_node(&self, painter: &Painter, layout: &Layout, node: &Node, look: NodeLook) {
        let accent = theme::category_color(layout.category());
        let dim = |color: Color32| if node.enabled { color } else { color.gamma_multiply(0.42) };
        let rect = layout.rect;
        let radius = CornerRadius::same(NODE_RADIUS);

        painter.add(theme::drop_shadow().as_shape(rect, radius));
        if look.glow > 0.0 || look.running {
            let strength = if look.running { look.glow.max(0.35) } else { look.glow };
            let halo =
                Shadow { offset: [0, 0], blur: 26, spread: 3, color: theme::with_alpha(accent, 0.75 * strength) };
            painter.add(halo.as_shape(rect, radius));
        }

        let body =
            if look.hovered { theme::mix(theme::bg_raised(), theme::bg_widget(), 0.5) } else { theme::bg_raised() };
        painter.rect_filled(rect, radius, body);
        let header = layout.header();
        let header_radius = CornerRadius { nw: NODE_RADIUS, ne: NODE_RADIUS, sw: 0, se: 0 };
        painter.rect_filled(header, header_radius, theme::mix(theme::bg_raised(), dim(accent), 0.2));
        painter.hline(rect.x_range(), header.bottom(), Stroke::new(1.0, theme::stroke()));

        painter.text(
            pos2(rect.left() + 11.0, header.center().y),
            Align2::LEFT_CENTER,
            node_icon(&node.kind),
            FontId::proportional(16.0),
            dim(accent),
        );
        let has_button = matches!(node.kind.as_str(), ids::START | ids::PLAY_RECORDING);
        let title_width = rect.width() - 36.0 - if has_button { 34.0 } else { 10.0 };
        let title = fit_text(painter, &layout.title, FontId::proportional(13.5), dim(theme::text()), title_width);
        painter.galley(pos2(rect.left() + 34.0, header.center().y - title.size().y / 2.0), title, theme::text());
        if !node.enabled {
            painter.text(
                pos2(rect.right() - if has_button { 40.0 } else { 12.0 }, header.center().y),
                Align2::RIGHT_CENTER,
                "off",
                FontId::proportional(10.5),
                theme::text_faint(),
            );
        }

        // Port labels.
        let label_font = FontId::proportional(11.5);
        let half = rect.width() / 2.0 - 16.0;
        for spot in &layout.spots {
            match (spot.port.kind, spot.is_input) {
                (PortKind::Data(_), true) => {
                    let text = fit_text(painter, &spot.port.label, label_font.clone(), dim(theme::text_dim()), half);
                    painter.galley(pos2(spot.pos.x + 12.0, spot.pos.y - text.size().y / 2.0), text, theme::text_dim());
                }
                (PortKind::Data(_), false) => {
                    let text = fit_text(painter, &spot.port.label, label_font.clone(), dim(theme::text_dim()), half);
                    let x = spot.pos.x - 12.0 - text.size().x;
                    painter.galley(pos2(x, spot.pos.y - text.size().y / 2.0), text, theme::text_dim());
                }
                (PortKind::Flow, false) if !spot.port.label.is_empty() => {
                    let text = fit_text(
                        painter,
                        &spot.port.label,
                        FontId::proportional(11.0),
                        dim(theme::text_dim()),
                        FLOW_SLOT - 10.0,
                    );
                    let x = spot.pos.x - text.size().x / 2.0;
                    painter.galley(pos2(x, spot.pos.y - 8.0 - text.size().y), text, theme::text_dim());
                }
                _ => {}
            }
        }

        // Summary lines: the first is the headline, the rest are detail.
        for (i, line) in layout.lines.iter().enumerate() {
            let color = if i == 0 { theme::text() } else { theme::text_dim() };
            let text = fit_text(painter, line, FontId::proportional(12.0), dim(color), rect.width() - 2.0 * PAD);
            painter.galley(pos2(rect.left() + PAD, layout.lines_top + i as f32 * LINE), text, color);
        }
        let _ = layout.rows_top;

        let outline = if look.error {
            Stroke::new(1.8, theme::danger())
        } else if look.selected {
            Stroke::new(1.8, theme::accent_bright())
        } else if look.running {
            Stroke::new(1.4, theme::with_alpha(accent, 0.9))
        } else {
            Stroke::new(1.0, theme::stroke_strong())
        };
        painter.rect_stroke(rect, radius, outline, StrokeKind::Outside);
    }

    fn node_menu(
        &mut self,
        ui: &mut Ui,
        project: &mut Project,
        registry: &Registry,
        node: &Node,
        events: &mut Vec<GraphEvent>,
    ) {
        if !self.selection.contains(&node.id) {
            self.selection = BTreeSet::from([node.id]);
        }
        let many = self.selection.len() > 1;
        if !many {
            if ui.button(format!("{}  Settings", icon::SLIDERS_HORIZONTAL)).clicked() {
                events.push(GraphEvent::Inspect(node.id));
                ui.close();
            }
            let is_flow = registry.ports(node).is_flow_node();
            if ui.add_enabled(is_flow, egui::Button::new(format!("{}  Run from here", icon::PLAY))).clicked() {
                events.push(GraphEvent::RunFrom(node.id));
                ui.close();
            }
            if node.kind == ids::PLAY_RECORDING {
                let asset = node.params_or_default::<PlayRecordingParams>().recording;
                let has = project.assets.recording(&asset).is_some();
                if ui
                    .add_enabled(has, egui::Button::new(format!("{}  Open recording editor", icon::ARROW_SQUARE_OUT)))
                    .clicked()
                {
                    events.push(GraphEvent::EditRecording(asset.clone()));
                    ui.close();
                }
                let convert = egui::Button::new(format!("{}  Convert to Smart Recording", icon::CROSSHAIR));
                if ui
                    .add_enabled(has, convert)
                    .on_hover_text(
                        "Movement made in a game that keeps re-centring the pointer becomes relative motion, \
                         so the camera turns when the recording plays.",
                    )
                    .clicked()
                {
                    if let Some(recording) = project.assets.recording_mut(&asset) {
                        let converted = recording.make_smart();
                        events.push(GraphEvent::Toast(match converted {
                            0 => "No game-camera movement found; the recording is unchanged.".to_owned(),
                            1 => "1 movement now replays as relative motion.".to_owned(),
                            n => format!("{n} movements now replay as relative motion."),
                        }));
                    }
                    ui.close();
                }
            }
            ui.separator();
        }
        let label = if node.enabled { "Disable" } else { "Enable" };
        if ui.button(format!("{}  {label}", icon::POWER)).clicked() {
            let enable = !node.enabled;
            for id in &self.selection {
                if let Some(target) = project.graph.node_mut(*id) {
                    target.enabled = enable;
                }
            }
            ui.close();
        }
        if ui.button(format!("{}  Duplicate", icon::COPY)).clicked() {
            self.duplicate_selection(project);
            ui.close();
        }
        if ui.button(format!("{}  Copy", icon::CLIPBOARD)).clicked() {
            self.copy_selection(ui, project, registry);
            ui.close();
        }
        ui.separator();
        let delete = if many { format!("Delete {} blocks", self.selection.len()) } else { "Delete".to_owned() };
        if ui.button(format!("{}  {delete}", icon::TRASH)).clicked() {
            self.delete_selection(project);
            ui.close();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn background(
        &mut self,
        ui: &Ui,
        painter: &Painter,
        project: &mut Project,
        env: &GraphEnv<'_>,
        background: &egui::Response,
        canvas: Rect,
        layouts: &[Layout],
        hovered_wire: Option<Wire>,
    ) {
        let from_screen = self.to_screen(canvas).inverse();
        let pointer = background.interact_pointer_pos().or(ui.ctx().pointer_latest_pos()).map(|p| from_screen * p);
        let modifiers = ui.input(|i| i.modifiers);
        let space_held = ui.input(|i| i.key_down(Key::Space));

        if background.clicked() {
            match (&hovered_wire, modifiers.alt) {
                (Some(wire), true) => {
                    project.graph.disconnect(wire);
                }
                _ if !modifiers.shift => self.selection.clear(),
                _ => {}
            }
            self.palette = None;
        }
        if background.double_clicked()
            && let Some(screen) = background.interact_pointer_pos()
        {
            self.open_palette(screen);
        }

        // Rubber-band selection.
        if background.drag_started_by(PointerButton::Primary) && !space_held {
            self.band_start = pointer;
        }
        if let (Some(start), Some(end)) = (self.band_start, pointer) {
            let band = Rect::from_two_pos(start, end);
            painter.rect_filled(band, 3.0, theme::with_alpha(theme::accent(), 0.12));
            painter.rect_stroke(band, 3.0, Stroke::new(1.0 / self.zoom, theme::accent_bright()), StrokeKind::Inside);
            if background.drag_stopped() {
                if !modifiers.shift {
                    self.selection.clear();
                }
                self.selection.extend(layouts.iter().filter(|l| l.rect.intersects(band)).map(|l| l.id));
            }
        }
        if !background.dragged_by(PointerButton::Primary) {
            self.band_start = None;
        }

        if background.secondary_clicked() {
            self.menu_pos = pointer.unwrap_or_default();
            self.menu_wire = hovered_wire;
        }
        background.context_menu(|ui| {
            if let Some(wire) = self.menu_wire.clone() {
                if ui.button(format!("{}  Delete wire", icon::SCISSORS)).clicked() {
                    project.graph.disconnect(&wire);
                    self.menu_wire = None;
                    ui.close();
                }
                ui.separator();
            }
            if ui.button(format!("{}  Add block…", icon::PLUS)).clicked() {
                let screen = self.to_screen(canvas) * self.menu_pos;
                self.open_palette(screen);
                ui.close();
            }
            if ui.button(format!("{}  Paste", icon::CLIPBOARD)).clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                ui.close();
            }
            if ui.button(format!("{}  Select all", icon::SELECTION)).clicked() {
                self.selection = project.graph.nodes().map(|n| n.id).collect();
                ui.close();
            }
            if ui.button(format!("{}  Fit to view", icon::CORNERS_OUT)).clicked() {
                self.fit_requested = true;
                ui.close();
            }
        });
        let _ = env;
    }

    /// Keyboard shortcuts, active while the pointer is over the canvas and no text is being typed.
    fn shortcuts(
        &mut self,
        ui: &Ui,
        project: &mut Project,
        registry: &Registry,
        canvas: Rect,
        background: &egui::Response,
    ) {
        if ui.ctx().text_edit_focused() || !Self::pointer_on_canvas(ui, canvas) {
            return;
        }
        let (delete, select_all, duplicate, fit, escape, copy, cut, paste) = ui.input(|i| {
            let paste = i.events.iter().find_map(|event| match event {
                egui::Event::Paste(text) => Some(text.clone()),
                _ => None,
            });
            (
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                i.modifiers.command && i.key_pressed(Key::A),
                i.modifiers.command && i.key_pressed(Key::D),
                i.key_pressed(Key::F) && !i.modifiers.any(),
                i.key_pressed(Key::Escape),
                i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
                i.events.iter().any(|e| matches!(e, egui::Event::Cut)),
                paste,
            )
        });
        if delete {
            self.delete_selection(project);
        }
        if select_all {
            self.selection = project.graph.nodes().map(|n| n.id).collect();
        }
        if duplicate {
            self.duplicate_selection(project);
        }
        if fit {
            self.fit_requested = true;
        }
        if escape {
            self.wire_drag = None;
            self.palette = None;
            self.selection.clear();
        }
        if copy || cut {
            self.copy_selection(ui, project, registry);
        }
        if cut {
            self.delete_selection(project);
        }
        if let Some(text) = paste {
            let at =
                background.hover_pos().map_or_else(|| self.view_center(), |p| self.to_screen(canvas).inverse() * p);
            self.paste(project, registry, &text, at);
        }
    }

    /// The searchable list of blocks that can be added. Arrow keys move
    /// through it, Enter adds the highlighted block; it can be resized.
    fn palette_popup(&mut self, ui: &Ui, project: &mut Project, registry: &Registry) {
        let Some(palette) = &mut self.palette else { return };
        let fresh = palette.just_opened;
        let mut chosen: Option<String> = None;
        let mut size = self.palette_size;

        // Everything that matches, in category order.
        let query = palette.query.trim().to_lowercase();
        let fits_wire = |def: &NodeDef| match &palette.wire {
            None => true,
            Some(drag) => match (drag.kind, drag.from_output) {
                (PortKind::Flow, true) => def.flow_in,
                (PortKind::Flow, false) => !def.flow_out.is_empty() || def.dynamic_ports.is_some(),
                (PortKind::Data(ty), true) => def.data_in.iter().any(|p| ty.fits(p.ty)) || def.dynamic_ports.is_some(),
                (PortKind::Data(ty), false) => def.data_out.iter().any(|p| p.ty.fits(ty)),
            },
        };
        let matches = |def: &NodeDef| {
            query.is_empty()
                || def.name.to_lowercase().contains(&query)
                || def.keywords.iter().any(|k| k.contains(&query))
                || def.category.label().to_lowercase().contains(&query)
        };
        let mut listed: Vec<Arc<NodeDef>> = Vec::new();
        for category in Category::ALL {
            listed.extend(
                registry.iter().filter(|def| def.category == category && matches(def) && fits_wire(def)).cloned(),
            );
        }

        // Arrow keys move the highlight; they are taken before the search box sees them.
        let (up, down, enter) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.key_pressed(Key::Enter),
            )
        });
        if !listed.is_empty() {
            if down {
                palette.highlight = (palette.highlight + 1).min(listed.len() - 1);
            }
            if up {
                palette.highlight = palette.highlight.saturating_sub(1);
            }
        }
        palette.highlight = palette.highlight.min(listed.len().saturating_sub(1));
        let moved = up || down;

        let area = egui::Area::new(Id::new("magi-palette"))
            .order(Order::Foreground)
            .fixed_pos(palette.screen_pos)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    let resize = egui::Resize::default()
                        .id_salt("magi-palette-size")
                        .default_size(size)
                        .min_size(vec2(220.0, 200.0))
                        .max_size(vec2(640.0, 900.0));
                    resize.show(ui, |ui| {
                        size = ui.available_size();
                        let search = ui.add(
                            egui::TextEdit::singleline(&mut palette.query)
                                .hint_text(format!("{}  Search blocks", icon::MAGNIFYING_GLASS))
                                .desired_width(f32::INFINITY),
                        );
                        if std::mem::take(&mut palette.just_opened) {
                            search.request_focus();
                        }
                        if search.changed() {
                            palette.highlight = 0;
                        }
                        ui.add_space(2.0);
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            let mut last_category = None;
                            for (index, def) in listed.iter().enumerate() {
                                if last_category != Some(def.category) {
                                    last_category = Some(def.category);
                                    ui.add_space(3.0);
                                    ui.label(
                                        egui::RichText::new(def.category.label().to_uppercase())
                                            .size(10.0)
                                            .color(theme::category_color(def.category)),
                                    );
                                }
                                let highlighted = index == palette.highlight;
                                let row = palette_row(ui, def, highlighted);
                                if highlighted && moved {
                                    row.scroll_to_me(None);
                                }
                                if row.hovered() && !moved {
                                    palette.highlight = index;
                                }
                                if row.on_hover_text(&def.description).clicked() {
                                    chosen = Some(def.id.clone());
                                }
                            }
                            if listed.is_empty() {
                                ui.weak("No block matches.");
                            }
                        });
                    });
                });
            });
        self.palette_size = size;

        if enter && chosen.is_none() {
            chosen = listed.get(palette.highlight).map(|def| def.id.clone());
        }
        let close = !fresh && (ui.input(|i| i.key_pressed(Key::Escape)) || area.response.clicked_elsewhere());

        if let Some(kind) = chosen {
            let palette = self.palette.take().expect("the palette is open");
            if let Some(id) = self.add_node(project, registry, &kind, palette.scene_pos - vec2(NODE_WIDTH / 2.0, 0.0))
                && let Some(drag) = palette.wire
            {
                connect_new_node(project, registry, &drag, id);
            }
        } else if close {
            self.palette = None;
        }
    }

    /// Zoom controls and the empty-canvas hint, floating over the canvas.
    fn overlay(&mut self, ui: &Ui, canvas: Rect, empty: bool) {
        if empty {
            ui.painter().text(
                canvas.center(),
                Align2::CENTER_CENTER,
                "Double-click or right-click to add a block",
                FontId::proportional(14.0),
                theme::text_faint(),
            );
        }
        egui::Area::new(Id::new("magi-zoom"))
            .order(Order::Middle)
            .fixed_pos(pos2(canvas.left() + 10.0, canvas.bottom() - 40.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(theme::with_alpha(theme::bg_panel(), 0.92))
                    .stroke(Stroke::new(1.0, theme::stroke()))
                    .corner_radius(8)
                    .inner_margin(3)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let small = |text: &str| egui::Button::new(text).frame(false).min_size(vec2(24.0, 22.0));
                            if ui.add(small(icon::MINUS)).on_hover_text("Zoom out").clicked() {
                                self.zoom_about(canvas, canvas.center(), 1.0 / 1.2);
                            }
                            let percent = format!("{:.0}%", self.zoom * 100.0);
                            if ui.add(small(&percent).min_size(vec2(44.0, 22.0))).on_hover_text("Reset zoom").clicked()
                            {
                                let factor = 1.0 / self.zoom;
                                self.zoom_about(canvas, canvas.center(), factor);
                            }
                            if ui.add(small(icon::PLUS)).on_hover_text("Zoom in").clicked() {
                                self.zoom_about(canvas, canvas.center(), 1.2);
                            }
                            if ui.add(small(icon::CORNERS_OUT)).on_hover_text("Fit everything in view (F)").clicked() {
                                self.fit_requested = true;
                            }
                        });
                    });
            });
    }

    /// The "recording information" card attached to a Play Recording block.
    #[allow(clippy::too_many_arguments)]
    fn recording_card(
        &mut self,
        scene: &mut Ui,
        painter: &Painter,
        project: &mut Project,
        layout: &Layout,
        node: &Node,
        rect: Rect,
        events: &mut Vec<GraphEvent>,
    ) {
        // The tether from the block to its card.
        let from = pos2(layout.rect.right(), layout.rect.top() + HEADER / 2.0);
        let to = pos2(rect.left(), from.y);
        painter.line_segment([from, to], Stroke::new(1.5, theme::stroke_strong()));
        painter.circle_filled(to, 3.0, theme::stroke_strong());

        // Swallow clicks and drags so they do not reach the canvas underneath.
        scene.interact(rect, scene.id().with(("card", node.id)), Sense::click_and_drag());
        painter.add(theme::drop_shadow().as_shape(rect, CornerRadius::same(NODE_RADIUS)));
        painter.rect_filled(rect, NODE_RADIUS, theme::bg_panel());
        painter.rect_stroke(rect, NODE_RADIUS, Stroke::new(1.0, theme::stroke_strong()), StrokeKind::Outside);

        let params: PlayRecordingParams = node.params_or_default();
        let mut card = scene.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink(10.0))
                .id_salt(("card-ui", node.id))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        card.set_clip_rect(rect.intersect(scene.clip_rect()));
        card.label(egui::RichText::new("RECORDING INFORMATION").size(10.0).color(theme::text_faint()));
        match project.assets.recording(&params.recording).cloned() {
            Some(recording) => {
                let response = recording_editor::show(&mut card, &recording, 120.0, false);
                if let Some(edit) = response.edit
                    && let Some(target) = project.assets.recording_mut(&params.recording)
                {
                    edit.apply(target);
                }
                if response.pop_out {
                    events.push(GraphEvent::EditRecording(params.recording.clone()));
                }
            }
            None => {
                card.add_space(6.0);
                card.weak("No recording selected yet. Record one with the Record button, or pick one in this block's settings.");
            }
        }
    }
}

/// One block in the block list: icon and name, left-aligned, one line tall.
fn palette_row(ui: &mut Ui, def: &NodeDef, highlighted: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if highlighted || response.hovered() {
            let fill = if highlighted { theme::with_alpha(theme::accent(), 0.28) } else { theme::bg_widget_hover() };
            painter.rect_filled(rect, 4.0, fill);
        }
        painter.text(
            pos2(rect.left() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            node_icon(&def.id),
            FontId::proportional(14.0),
            theme::category_color(def.category),
        );
        let name = fit_text(painter, &def.name, FontId::proportional(13.0), theme::text(), rect.width() - 36.0);
        painter.galley(pos2(rect.left() + 30.0, rect.center().y - name.size().y / 2.0), name, theme::text());
    }
    response
}

/// How a node should be drawn this frame.
struct NodeLook {
    selected: bool,
    hovered: bool,
    /// 1.0 right after the node ran, fading to 0.
    glow: f32,
    running: bool,
    error: bool,
}

/// Attaches a dangling wire to the first port of a new node that accepts it.
fn connect_new_node(project: &mut Project, registry: &Registry, drag: &WireDrag, new_node: NodeId) {
    let Some(node) = project.graph.node(new_node) else { return };
    let node_ports = registry.ports(node);
    let candidates: Vec<PortRef> = if drag.from_output {
        let flow_in = node_ports.flow_in.then(|| PortRef::new(new_node, ports::FLOW_IN));
        flow_in.into_iter().chain(node_ports.data_in.iter().map(|p| PortRef::new(new_node, p.id.clone()))).collect()
    } else {
        node_ports.flow_out.iter().chain(&node_ports.data_out).map(|p| PortRef::new(new_node, p.id.clone())).collect()
    };
    for candidate in candidates {
        let (from, to) = if drag.from_output { (drag.port.clone(), candidate) } else { (candidate, drag.port.clone()) };
        if project.graph.connect(registry, from, to).is_ok() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    //! The editor driven headlessly: a simulated pointer and keyboard, no window.

    use egui::Modifiers;
    use egui_kittest::Harness;

    use super::*;

    struct World {
        view: GraphView,
        project: Project,
        registry: Registry,
        events: Vec<GraphEvent>,
    }

    impl World {
        /// A Start block above a Wait block, not connected.
        fn new() -> (Self, NodeId, NodeId) {
            let registry = Registry::builtin();
            let mut project = Project::new();
            let start = project.graph.add(&registry, ids::START, [0.0, 0.0]).unwrap();
            let wait = project.graph.add(&registry, ids::DELAY, [0.0, 200.0]).unwrap();
            (Self { view: GraphView::default(), project, registry, events: Vec::new() }, start, wait)
        }

        fn layout(&self, node: NodeId) -> Layout {
            Layout::compute(self.project.graph.node(node).unwrap(), &self.project, &self.registry)
        }

        /// Where a port is on screen right now.
        fn port(&self, node: NodeId, port: &str, is_input: bool) -> Pos2 {
            let layout = self.layout(node);
            self.view.to_screen(self.view.canvas) * layout.spot(port, is_input).unwrap().pos
        }

        /// A point on a block's body: below the header, clear of every port.
        fn body(&self, node: NodeId) -> Pos2 {
            let rect = self.layout(node).rect;
            self.view.to_screen(self.view.canvas) * pos2(rect.left() + 40.0, rect.top() + HEADER + 14.0)
        }
    }

    fn harness(world: World) -> Harness<'static, World> {
        // Steps of 1/60 s (the default is 1/4 s), so two clicks can be a double-click.
        let builder = Harness::builder().with_size(vec2(900.0, 700.0)).with_step_dt(1.0 / 60.0).with_max_steps(12);
        let mut harness = builder.build_ui_state(
            |ui, world: &mut World| {
                let activity = HashMap::new();
                let env = GraphEnv { registry: &world.registry, activity: &activity, running: &[], error_node: None };
                let events = world.view.show(ui, &mut world.project, &env);
                world.events.extend(events);
            },
            world,
        );
        harness.run();
        harness
    }

    /// Presses at `from`, moves to `to` in a few steps, releases.
    fn drag(harness: &mut Harness<'_, World>, from: Pos2, to: Pos2) {
        harness.hover_at(from);
        harness.run();
        harness.drag_at(from);
        harness.run();
        for step in 1..=4 {
            harness.hover_at(from + (to - from) * (step as f32 / 4.0));
            harness.run();
        }
        harness.drop_at(to);
        harness.run();
    }

    fn button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    /// Clicks and leaves the pointer where it is, as a real mouse would.
    /// (`Harness::drop_at` also takes the pointer away.)
    fn click(harness: &mut Harness<'_, World>, at: Pos2) {
        harness.hover_at(at);
        harness.step();
        harness.event(button(at, true));
        harness.step();
        harness.event(button(at, false));
        harness.step();
    }

    #[test]
    fn dragging_from_an_output_to_an_input_wires_them() {
        let (world, start, wait) = World::new();
        let mut harness = harness(world);
        let (from, to) =
            (harness.state().port(start, ports::OUT, false), harness.state().port(wait, ports::FLOW_IN, true));
        drag(&mut harness, from, to);

        let graph = &harness.state().project.graph;
        assert_eq!(graph.flow_target(start, ports::OUT).map(|t| t.node), Some(wait));
    }

    #[test]
    fn wires_can_be_drawn_backwards_from_the_input() {
        let (world, start, wait) = World::new();
        let mut harness = harness(world);
        let (from, to) =
            (harness.state().port(wait, ports::FLOW_IN, true), harness.state().port(start, ports::OUT, false));
        drag(&mut harness, from, to);
        assert_eq!(harness.state().project.graph.wires().len(), 1);
    }

    #[test]
    fn incompatible_ports_do_not_connect() {
        let (mut world, start, _) = World::new();
        let find = world.project.graph.add(&world.registry, ids::FIND_IMAGE, [320.0, 0.0]).unwrap();
        let mut harness = harness(world);
        // A flow output cannot feed... anything on a block that only has value outputs.
        let (from, to) = (harness.state().port(start, ports::OUT, false), harness.state().port(find, "found", false));
        drag(&mut harness, from, to);
        assert!(harness.state().project.graph.wires().is_empty());
    }

    #[test]
    fn dragging_a_block_moves_and_selects_it() {
        let (world, _, wait) = World::new();
        let mut harness = harness(world);
        let before = harness.state().project.graph.node(wait).unwrap().pos;
        let grab = harness.state().body(wait);
        drag(&mut harness, grab, grab + vec2(120.0, 60.0));

        let world = harness.state();
        let after = world.project.graph.node(wait).unwrap().pos;
        let moved = vec2(after[0] - before[0], after[1] - before[1]) * world.view.zoom;
        assert!((moved - vec2(120.0, 60.0)).length() < 12.0, "moved by {moved:?}");
        assert_eq!(world.view.selection, BTreeSet::from([wait]));
    }

    #[test]
    fn clicking_selects_and_delete_removes() {
        let (world, start, wait) = World::new();
        let mut harness = harness(world);
        let at = harness.state().body(wait);
        click(&mut harness, at);
        assert_eq!(harness.state().view.selection, BTreeSet::from([wait]));

        // The pointer is still over the block: shortcuts must work there too.
        harness.key_press(Key::Delete);
        harness.run();
        let graph = &harness.state().project.graph;
        assert!(graph.contains(start) && !graph.contains(wait));
    }

    #[test]
    fn the_wheel_zooms_about_the_pointer_even_over_a_block() {
        let (world, _, wait) = World::new();
        let mut harness = harness(world);
        let at = harness.state().body(wait);
        let before = harness.state().view.zoom;
        let anchor = harness.state().view.to_screen(harness.state().view.canvas).inverse() * at;

        harness.hover_at(at);
        harness.step();
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, -3.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        harness.run_steps(3);

        let world = harness.state();
        assert!(world.view.zoom < before, "scrolling down zooms out ({} -> {})", before, world.view.zoom);
        let after = world.view.to_screen(world.view.canvas) * anchor;
        assert!(after.distance(at) < 1.5, "the point under the pointer stays put");
    }

    #[test]
    fn a_band_dragged_over_blocks_selects_them() {
        let (world, start, wait) = World::new();
        let mut harness = harness(world);
        let canvas = harness.state().view.canvas;
        drag(&mut harness, canvas.min + vec2(20.0, 20.0), canvas.max - vec2(20.0, 60.0));
        assert_eq!(harness.state().view.selection, BTreeSet::from([start, wait]));

        // Clicking empty canvas clears the selection again.
        click(&mut harness, canvas.min + vec2(20.0, 20.0));
        assert!(harness.state().view.selection.is_empty());
    }

    #[test]
    fn select_all_and_duplicate_use_the_keyboard() {
        let (world, ..) = World::new();
        let mut harness = harness(world);
        let canvas = harness.state().view.canvas;
        harness.hover_at(canvas.min + vec2(20.0, 20.0));
        harness.run();
        harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
        harness.run();
        assert_eq!(harness.state().view.selection.len(), 2);
        harness.key_press_modifiers(Modifiers::COMMAND, Key::D);
        harness.run();
        assert_eq!(harness.state().project.graph.len(), 4);
        assert_eq!(harness.state().view.selection.len(), 2, "the copies are selected");
    }

    #[test]
    fn dropping_a_wire_on_empty_canvas_offers_blocks_and_wires_the_choice() {
        let (world, start, _) = World::new();
        let mut harness = harness(world);
        let from = harness.state().port(start, ports::OUT, false);
        let canvas = harness.state().view.canvas;
        drag(&mut harness, from, canvas.max - vec2(120.0, 120.0));
        assert!(harness.state().view.palette.is_some(), "the block list opens where the wire was dropped");

        // Type into the search box and accept the first match.
        harness.event(egui::Event::Text("log".to_owned()));
        harness.run();
        harness.key_press(Key::Enter);
        harness.run();

        let world = harness.state();
        let log = world.project.graph.nodes().find(|n| n.kind == ids::LOG).expect("a Log block was added").id;
        assert_eq!(world.project.graph.flow_target(start, ports::OUT).map(|t| t.node), Some(log));
        assert!(world.view.palette.is_none());
    }

    #[test]
    fn the_button_on_a_start_block_asks_to_run_it() {
        let (world, start, _) = World::new();
        let mut harness = harness(world);
        let button = {
            let world = harness.state();
            world.view.to_screen(world.view.canvas) * world.layout(start).header_button().center()
        };
        click(&mut harness, button);
        assert!(harness.state().events.iter().any(|e| matches!(e, GraphEvent::ToggleStart(id) if *id == start)));
    }

    #[test]
    fn double_clicking_a_block_asks_for_its_settings() {
        let (world, _, wait) = World::new();
        let mut harness = harness(world);
        let at = harness.state().body(wait);
        click(&mut harness, at);
        assert!(harness.state().events.is_empty(), "a single click only selects");
        click(&mut harness, at); // 1/60 s steps keep this within the double-click time
        assert!(harness.state().events.iter().any(|e| matches!(e, GraphEvent::Inspect(id) if *id == wait)));
    }

    #[test]
    fn copy_and_paste_round_trip_through_text() {
        let (mut world, start, wait) = World::new();
        world
            .project
            .graph
            .connect(&world.registry, PortRef::new(start, ports::OUT), PortRef::new(wait, ports::FLOW_IN))
            .unwrap();
        let preset = NodePreset::capture(
            &world.project.graph,
            &BTreeSet::from([start, wait]),
            &world.project.assets,
            &world.registry,
        );
        let text = format!("{CLIPBOARD_TAG}{}", serde_json::to_string(&preset).unwrap());

        let World { mut view, mut project, registry, .. } = world;
        assert!(view.paste(&mut project, &registry, &text, pos2(400.0, 400.0)));
        assert_eq!(project.graph.len(), 4);
        assert_eq!(project.graph.wires().len(), 2, "the wire between the copied blocks is copied too");
        assert!(
            !view.paste(&mut project, &registry, "just some text", Pos2::ZERO),
            "foreign clipboard text is ignored"
        );
    }
}
