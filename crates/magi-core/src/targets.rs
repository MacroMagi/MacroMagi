//! The places on screen a macro touches: click positions and search areas,
//! in the order a run reaches them.
//!
//! Used by the editor's *View › Show on screen* overlay, which numbers them
//! and lets them be dragged into place.

use std::collections::{BTreeSet, HashSet};

use crate::action::Action;
use crate::autoclicker::{AutoClickParams, ClickPosition};
use crate::geom::{Point, Rect};
use crate::graph::{Graph, NodeId};
use crate::nodes::vision::FindImageParams;
use crate::nodes::{ParamKind, Registry, ids};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetShape {
    Point(Point),
    Region(Rect),
}

/// Where a target's value is stored, so an edit can be written back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetSource {
    /// A `Point` or `Region` parameter of a block.
    Param { node: NodeId, key: String },
    /// The position of one input in a list of inputs (an Input block's
    /// `actions`, a Rate block's `steps`).
    Action { node: NodeId, key: String, index: usize },
    /// The fixed position of an Auto Click block.
    Clicker { node: NodeId },
    /// The search area of one image of an Image Detection block.
    Image { node: NodeId, index: usize },
    /// The search area of one image group of an Image Detection block.
    ImageGroup { node: NodeId, index: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScreenTarget {
    /// 1-based position in run order.
    pub order: usize,
    pub node: NodeId,
    pub label: String,
    pub shape: TargetShape,
    pub source: TargetSource,
}

/// Every position and area the macro uses, numbered in run order. Blocks no
/// Start block reaches come last, in id order.
pub fn screen_targets(graph: &Graph, registry: &Registry) -> Vec<ScreenTarget> {
    let mut visited = Vec::new();
    let mut seen = HashSet::new();
    for start in graph.starts() {
        visit(graph, registry, start.id, &mut visited, &mut seen);
    }
    let all: BTreeSet<NodeId> = graph.nodes().map(|n| n.id).collect();
    visited.extend(all.into_iter().filter(|id| seen.insert(*id)));

    let mut targets = Vec::new();
    for id in visited {
        let Some(node) = graph.node(id) else { continue };
        let title = registry.title(node);
        let mut push = |label: String, shape, source| {
            targets.push(ScreenTarget { order: targets.len() + 1, node: id, label, shape, source })
        };
        match node.kind.as_str() {
            ids::AUTOCLICK => {
                let params: AutoClickParams = node.params_or_default();
                if let Some(point) = params.clicker.position.point() {
                    push(title.clone(), TargetShape::Point(point), TargetSource::Clicker { node: id });
                }
            }
            ids::FIND_IMAGE => {
                let params: FindImageParams = node.params_or_default();
                if let Some(region) = params.region {
                    push(
                        title.clone(),
                        TargetShape::Region(region),
                        TargetSource::Param { node: id, key: "region".into() },
                    );
                }
                for (index, group) in params.groups.iter().enumerate() {
                    if let Some(region) = group.region {
                        let label = format!("{title} – group “{}”", group.name);
                        push(label, TargetShape::Region(region), TargetSource::ImageGroup { node: id, index });
                    }
                }
                for (index, target) in params.targets().iter().enumerate() {
                    if let Some(region) = target.region {
                        let label = format!("{title} – image {}", index + 1);
                        push(label, TargetShape::Region(region), TargetSource::Image { node: id, index });
                    }
                }
            }
            _ => {
                let Some(def) = registry.get(&node.kind) else { continue };
                for param in &def.params {
                    if param.kind == ParamKind::Actions {
                        let actions: Vec<Action> = node.param(&param.key).unwrap_or_default();
                        for (index, action) in actions.iter().enumerate() {
                            let point = match action {
                                Action::Mouse { at: Some(p), .. } => Some(*p),
                                Action::Move { x, y, display, relative: false, .. } => {
                                    Some(Point { x: *x, y: *y, display: *display })
                                }
                                _ => None,
                            };
                            if let Some(point) = point {
                                let source = TargetSource::Action { node: id, key: param.key.clone(), index };
                                push(title.clone(), TargetShape::Point(point), source);
                            }
                        }
                        continue;
                    }
                    let shape = match param.kind {
                        ParamKind::Point => node.param::<Option<Point>>(&param.key).flatten().map(TargetShape::Point),
                        ParamKind::Region => node.param::<Option<Rect>>(&param.key).flatten().map(TargetShape::Region),
                        _ => None,
                    };
                    if let Some(shape) = shape {
                        push(title.clone(), shape, TargetSource::Param { node: id, key: param.key.clone() });
                    }
                }
            }
        }
    }
    targets
}

/// Depth-first along flow wires; the value blocks a flow block reads come just before it.
fn visit(graph: &Graph, registry: &Registry, id: NodeId, out: &mut Vec<NodeId>, seen: &mut HashSet<NodeId>) {
    if !seen.insert(id) {
        return;
    }
    let Some(node) = graph.node(id) else { return };
    let node_ports = registry.ports(node);
    for input in &node_ports.data_in {
        if let Some(source) = graph.data_source(id, &input.id) {
            visit_values(graph, registry, source.node, out, seen);
        }
    }
    out.push(id);
    for output in &node_ports.flow_out {
        if let Some(target) = graph.flow_target(id, &output.id) {
            visit(graph, registry, target.node, out, seen);
        }
    }
}

fn visit_values(graph: &Graph, registry: &Registry, id: NodeId, out: &mut Vec<NodeId>, seen: &mut HashSet<NodeId>) {
    if !seen.insert(id) {
        return;
    }
    if let Some(node) = graph.node(id) {
        for input in &registry.ports(node).data_in {
            if let Some(source) = graph.data_source(id, &input.id) {
                visit_values(graph, registry, source.node, out, seen);
            }
        }
    }
    out.push(id);
}

/// Writes a moved or resized target back into the graph.
pub fn apply(graph: &mut Graph, source: &TargetSource, shape: TargetShape) {
    match source {
        TargetSource::Param { node, key } => {
            if let Some(node) = graph.node_mut(*node) {
                let value = match shape {
                    TargetShape::Point(p) => serde_json::to_value(p),
                    TargetShape::Region(r) => serde_json::to_value(r),
                };
                if let Ok(value) = value {
                    node.params.insert(key.clone(), value);
                }
            }
        }
        TargetSource::Action { node, key, index } => {
            let (Some(node), TargetShape::Point(p)) = (graph.node_mut(*node), shape) else { return };
            let mut actions: Vec<Action> = node.param(key).unwrap_or_default();
            match actions.get_mut(*index) {
                Some(Action::Mouse { at, .. }) => *at = Some(p),
                Some(Action::Move { x, y, display, .. }) => (*x, *y, *display) = (p.x, p.y, p.display),
                _ => return,
            }
            node.set_param(key, actions);
        }
        TargetSource::Image { node, index } => {
            let (Some(node), TargetShape::Region(r)) = (graph.node_mut(*node), shape) else { return };
            let params: FindImageParams = node.params_or_default();
            let mut images = params.targets();
            if let Some(target) = images.get_mut(*index) {
                target.region = Some(r);
                node.set_param("images", images);
                node.set_param("image", "");
            }
        }
        TargetSource::ImageGroup { node, index } => {
            let (Some(node), TargetShape::Region(r)) = (graph.node_mut(*node), shape) else { return };
            let mut params: FindImageParams = node.params_or_default();
            if let Some(group) = params.groups.get_mut(*index) {
                group.region = Some(r);
                node.set_param("groups", params.groups);
            }
        }
        TargetSource::Clicker { node } => {
            let (Some(node), TargetShape::Point(p)) = (graph.node_mut(*node), shape) else { return };
            let mut params: AutoClickParams = node.params_or_default();
            params.clicker.position = ClickPosition::Fixed { x: p.x, y: p.y, display: p.display };
            node.set_params(&params);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::graph::PortRef;
    use crate::nodes::ports;

    #[test]
    fn targets_are_numbered_in_run_order_and_edits_write_back() {
        let registry = Registry::builtin();
        let mut graph = Graph::new();
        let start = graph.add(&registry, ids::START, [0.0, 0.0]).unwrap();
        let second = graph.add(&registry, ids::INPUT, [0.0, 0.0]).unwrap();
        let first = graph.add(&registry, ids::INPUT, [0.0, 0.0]).unwrap();
        let find = graph.add(&registry, ids::FIND_IMAGE, [0.0, 0.0]).unwrap();
        graph
            .node_mut(first)
            .unwrap()
            .set_param("actions", json!([{ "kind": "mouse", "button": "Left", "at": { "x": 1, "y": 2 } }]));
        graph.node_mut(second).unwrap().set_param("actions", json!([{ "kind": "move", "x": 5, "y": 6 }]));
        graph.node_mut(find).unwrap().set_param("region", Rect::new(10, 10, 50, 40));
        graph.connect(&registry, PortRef::new(start, ports::OUT), PortRef::new(first, ports::FLOW_IN)).unwrap();
        graph.connect(&registry, PortRef::new(first, ports::OUT), PortRef::new(second, ports::FLOW_IN)).unwrap();
        graph.connect(&registry, PortRef::new(find, "position"), PortRef::new(second, "position")).unwrap();

        let targets = screen_targets(&graph, &registry);
        let order: Vec<NodeId> = targets.iter().map(|t| t.node).collect();
        assert_eq!(order, [first, find, second], "the image search happens just before the block that reads it");
        assert_eq!(targets[1].shape, TargetShape::Region(Rect::new(10, 10, 50, 40)));

        apply(&mut graph, &targets[0].source, TargetShape::Point(Point::new(100, 200)));
        let moved = screen_targets(&graph, &registry);
        assert_eq!(moved[0].shape, TargetShape::Point(Point::new(100, 200)));
    }
}
