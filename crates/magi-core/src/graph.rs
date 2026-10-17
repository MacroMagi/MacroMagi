//! The node graph: nodes, the wires between them, and the rules for editing them.
//!
//! Two kinds of wire exist. **Flow** wires say what happens next and run from a
//! flow output to a flow input; an output leads to exactly one place. **Data**
//! wires carry a value from a data output to a data input; an input has exactly
//! one source. [`Graph::connect`] enforces both, replacing an old wire when a
//! new one takes its place.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::nodes::{PortKind, Registry, ids};

pub type NodeId = u64;

fn yes() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    /// Node type id, e.g. `flow.delay`. See [`crate::nodes`].
    #[serde(rename = "type")]
    pub kind: String,
    /// Position of the node's top-left corner on the canvas.
    pub pos: [f32; 2],
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub params: Map<String, Value>,
    /// Disabled flow nodes are skipped; disabled value nodes produce nothing.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// Custom title. Empty means "use the type's name".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

impl Node {
    pub fn new(id: NodeId, kind: &str, pos: [f32; 2], params: Map<String, Value>) -> Self {
        Self { id, kind: kind.to_owned(), pos, params, enabled: true, label: String::new() }
    }

    /// Reads one parameter as a typed value.
    pub fn param<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.params.get(key).and_then(|value| T::deserialize(value).ok())
    }

    /// Reads all parameters as a typed struct.
    pub fn params_as<T: DeserializeOwned>(&self) -> Result<T> {
        Ok(T::deserialize(&Value::Object(self.params.clone()))?)
    }

    /// Like [`Self::params_as`], falling back to defaults if the stored
    /// parameters are malformed. For display code that must not fail.
    pub fn params_or_default<T: DeserializeOwned + Default>(&self) -> T {
        self.params_as().unwrap_or_default()
    }

    pub fn set_param(&mut self, key: &str, value: impl Serialize) {
        if let Ok(value) = serde_json::to_value(value) {
            self.params.insert(key.to_owned(), value);
        }
    }

    /// Overwrites the parameters present in `params`, leaving others untouched.
    pub fn set_params<T: Serialize>(&mut self, params: &T) {
        if let Ok(Value::Object(map)) = serde_json::to_value(params) {
            self.params.extend(map);
        }
    }

    pub fn is_start(&self) -> bool {
        self.kind == ids::START
    }
}

/// One end of a wire.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PortRef {
    pub node: NodeId,
    pub port: String,
}

impl PortRef {
    pub fn new(node: NodeId, port: impl Into<String>) -> Self {
        Self { node, port: port.into() }
    }
}

/// A connection from an output port to an input port.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Wire {
    pub from: PortRef,
    pub to: PortRef,
}

/// A named, colored box around some blocks. Disabling a group disables
/// every block in it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: u64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    /// The blocks directly in this group – not those of the groups inside it.
    /// A block is directly in one group at most.
    pub nodes: BTreeSet<NodeId>,
    /// The group this one is inside.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

/// A self-contained piece of a graph: what copy/paste, duplicate and node
/// presets move around. Wires are only those between the contained nodes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Fragment {
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub wires: Vec<Wire>,
    /// Groups whose blocks are all in the fragment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
}

impl Fragment {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Top-left corner of the fragment's nodes.
    pub fn origin(&self) -> [f32; 2] {
        self.nodes.iter().fold([f32::INFINITY, f32::INFINITY], |acc, n| [acc[0].min(n.pos[0]), acc[1].min(n.pos[1])])
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "Fragment", into = "Fragment")]
pub struct Graph {
    nodes: BTreeMap<NodeId, Node>,
    wires: Vec<Wire>,
    groups: Vec<Group>,
    next_id: NodeId,
}

impl From<Fragment> for Graph {
    fn from(file: Fragment) -> Self {
        let next_id = file.nodes.iter().map(|n| n.id + 1).max().unwrap_or(1);
        let nodes: BTreeMap<NodeId, Node> = file.nodes.into_iter().map(|n| (n.id, n)).collect();
        // Drop wires that point at nodes which are not there, and exact duplicates.
        let mut seen = BTreeSet::new();
        let wires = file
            .wires
            .into_iter()
            .filter(|w| nodes.contains_key(&w.from.node) && nodes.contains_key(&w.to.node))
            .filter(|w| seen.insert((w.from.clone(), w.to.clone())))
            .collect();
        let groups = file
            .groups
            .into_iter()
            .map(|mut g| {
                g.nodes.retain(|id| nodes.contains_key(id));
                g
            })
            .collect();
        let mut graph = Graph { nodes, wires, groups, next_id };
        graph.nest_overlapping_groups();
        graph.prune_groups();
        graph
    }
}

impl From<Graph> for Fragment {
    fn from(graph: Graph) -> Self {
        Fragment { nodes: graph.nodes.into_values().collect(), wires: graph.wires, groups: graph.groups }
    }
}

impl Graph {
    pub fn new() -> Self {
        Self { next_id: 1, ..Self::default() }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    pub fn nodes_mut(&mut self) -> impl Iterator<Item = &mut Node> {
        self.nodes.values_mut()
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(&id)
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    pub fn wires(&self) -> &[Wire] {
        &self.wires
    }

    /// Start blocks, in id order.
    pub fn starts(&self) -> impl Iterator<Item = &Node> {
        self.nodes().filter(|n| n.is_start())
    }

    fn fresh_id(&mut self) -> NodeId {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    /// Adds a node of a registered type with its default parameters.
    pub fn add(&mut self, registry: &Registry, kind: &str, pos: [f32; 2]) -> Result<NodeId> {
        let def = registry.get(kind).ok_or_else(|| Error::graph(format!("unknown node type `{kind}`")))?;
        Ok(self.add_with(kind, pos, def.default_params()))
    }

    /// Adds a node with explicit parameters, without consulting a registry.
    pub fn add_with(&mut self, kind: &str, pos: [f32; 2], params: Map<String, Value>) -> NodeId {
        let id = self.fresh_id();
        self.nodes.insert(id, Node::new(id, kind, pos, params));
        id
    }

    /// Removes a node and every wire attached to it.
    pub fn remove(&mut self, id: NodeId) -> Option<Node> {
        let node = self.nodes.remove(&id)?;
        self.wires.retain(|w| w.from.node != id && w.to.node != id);
        for group in &mut self.groups {
            group.nodes.remove(&id);
        }
        self.prune_groups();
        Some(node)
    }

    // ---- groups -------------------------------------------------------------

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    pub fn group(&self, id: u64) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn group_mut(&mut self, id: u64) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// The group a block is directly in.
    pub fn group_of(&self, node: NodeId) -> Option<u64> {
        self.groups.iter().find(|g| g.nodes.contains(&node)).map(|g| g.id)
    }

    /// A group and every group inside it, at any depth.
    pub fn group_and_inner(&self, id: u64) -> Vec<u64> {
        let mut found = vec![id];
        let mut index = 0;
        while index < found.len() {
            let current = found[index];
            let inner: Vec<u64> = self
                .groups
                .iter()
                .filter(|g| g.parent == Some(current) && !found.contains(&g.id))
                .map(|g| g.id)
                .collect();
            found.extend(inner);
            index += 1;
        }
        found
    }

    /// Every block in a group, those of the groups inside it included.
    pub fn group_blocks(&self, id: u64) -> BTreeSet<NodeId> {
        self.group_and_inner(id).iter().filter_map(|g| self.group(*g)).flat_map(|g| g.nodes.iter().copied()).collect()
    }

    /// How many groups a group is inside.
    pub fn group_depth(&self, id: u64) -> usize {
        let mut depth = 0;
        let mut current = self.group(id).and_then(|g| g.parent);
        while let Some(parent) = current {
            depth += 1;
            if depth > self.groups.len() {
                break;
            }
            current = self.group(parent).and_then(|g| g.parent);
        }
        depth
    }

    /// Puts the given blocks in a new group. Returns its id.
    pub fn add_group(&mut self, name: &str, nodes: &BTreeSet<NodeId>) -> Option<u64> {
        self.add_group_around(name, nodes, &BTreeSet::new())
    }

    /// Puts blocks and groups in a new group, which takes their place: when
    /// they were all in the same group, the new one is inside that group.
    /// Returns its id.
    pub fn add_group_around(&mut self, name: &str, nodes: &BTreeSet<NodeId>, groups: &BTreeSet<u64>) -> Option<u64> {
        // A selected group's own contents move with it.
        let groups: BTreeSet<u64> = groups
            .iter()
            .copied()
            .filter(|g| self.group(*g).is_some())
            .filter(|g| !groups.iter().any(|other| other != g && self.group_and_inner(*other).contains(g)))
            .collect();
        let inside: BTreeSet<NodeId> = groups.iter().flat_map(|g| self.group_blocks(*g)).collect();
        let nodes: BTreeSet<NodeId> =
            nodes.iter().copied().filter(|id| self.nodes.contains_key(id) && !inside.contains(id)).collect();
        if nodes.is_empty() && groups.is_empty() {
            return None;
        }
        let containers: BTreeSet<Option<u64>> = nodes
            .iter()
            .map(|n| self.group_of(*n))
            .chain(groups.iter().map(|g| self.group(*g).and_then(|g| g.parent)))
            .collect();
        let parent = if containers.len() == 1 { containers.into_iter().next().flatten() } else { None };
        let id = self.groups.iter().map(|g| g.id + 1).max().unwrap_or(1);
        for group in &mut self.groups {
            group.nodes.retain(|n| !nodes.contains(n));
            if groups.contains(&group.id) {
                group.parent = Some(id);
            }
        }
        self.groups.push(Group { id, name: name.to_owned(), color: None, nodes, parent, enabled: true });
        self.prune_groups();
        Some(id)
    }

    /// Removes a group; its blocks and the groups inside it move out one level.
    pub fn remove_group(&mut self, id: u64) {
        let Some(index) = self.groups.iter().position(|g| g.id == id) else { return };
        let group = self.groups.remove(index);
        if let Some(parent) = group.parent.and_then(|p| self.group_mut(p)) {
            parent.nodes.extend(group.nodes);
        }
        for inner in self.groups.iter_mut().filter(|g| g.parent == Some(id)) {
            inner.parent = group.parent;
        }
        self.prune_groups();
    }

    /// Takes a block out of the group it is in, into the group around that
    /// one, if there is one.
    pub fn take_out_of_group(&mut self, node: NodeId) {
        let Some(id) = self.group_of(node) else { return };
        let parent = self.group(id).and_then(|g| g.parent);
        if let Some(group) = self.group_mut(id) {
            group.nodes.remove(&node);
        }
        if let Some(parent) = parent.and_then(|p| self.group_mut(p)) {
            parent.nodes.insert(node);
        }
        self.prune_groups();
    }

    /// Moves blocks and groups into a group. A group cannot go inside itself
    /// or a group inside it. Returns whether anything moved.
    pub fn move_into_group(&mut self, target: u64, nodes: &BTreeSet<NodeId>, groups: &BTreeSet<u64>) -> bool {
        if self.group(target).is_none() {
            return false;
        }
        let mut moved = false;
        for &node in nodes {
            if !self.nodes.contains_key(&node) || self.group_of(node) == Some(target) {
                continue;
            }
            for group in &mut self.groups {
                group.nodes.remove(&node);
            }
            if let Some(group) = self.group_mut(target) {
                group.nodes.insert(node);
            }
            moved = true;
        }
        for &id in groups {
            if self.group_and_inner(id).contains(&target) {
                continue;
            }
            if let Some(group) = self.group_mut(id)
                && group.parent != Some(target)
            {
                group.parent = Some(target);
                moved = true;
            }
        }
        self.prune_groups();
        moved
    }

    /// Whether a block runs: it is enabled, and so is every group around it.
    pub fn is_active(&self, id: NodeId) -> bool {
        if !self.nodes.get(&id).is_some_and(|n| n.enabled) {
            return false;
        }
        let mut current = self.group_of(id);
        let mut steps = 0;
        while let Some(group) = current.and_then(|g| self.group(g)) {
            if !group.enabled {
                return false;
            }
            steps += 1;
            if steps > self.groups.len() {
                break;
            }
            current = group.parent;
        }
        true
    }

    /// A copy in which blocks of disabled groups are themselves disabled,
    /// which is all the engines need to know about groups.
    pub fn with_groups_applied(&self) -> Graph {
        let mut graph = self.clone();
        for group in self.groups.iter().filter(|g| !g.enabled) {
            for id in self.group_blocks(group.id) {
                if let Some(node) = graph.nodes.get_mut(&id) {
                    node.enabled = false;
                }
            }
        }
        graph
    }

    /// Keeps the groups a tree: parents that are missing (or would make a
    /// loop) are dropped, and so are groups left with nothing in them.
    fn prune_groups(&mut self) {
        loop {
            let ids: BTreeSet<u64> = self.groups.iter().map(|g| g.id).collect();
            let parents: HashMap<u64, Option<u64>> = self.groups.iter().map(|g| (g.id, g.parent)).collect();
            for group in &mut self.groups {
                if group.parent.is_some_and(|p| !ids.contains(&p)) {
                    group.parent = None;
                }
                // A chain that comes back to the group is cut here.
                let mut current = group.parent;
                for _ in 0..=ids.len() {
                    match current {
                        Some(p) if p == group.id => {
                            group.parent = None;
                            break;
                        }
                        Some(p) => current = parents.get(&p).copied().flatten(),
                        None => break,
                    }
                }
            }
            let empty: Vec<u64> = self
                .groups
                .iter()
                .filter(|g| g.nodes.is_empty() && !self.groups.iter().any(|other| other.parent == Some(g.id)))
                .map(|g| g.id)
                .collect();
            if empty.is_empty() {
                break;
            }
            self.groups.retain(|g| !empty.contains(&g.id));
        }
    }

    /// Files from before groups could nest let a block be in several groups.
    /// The block stays in the smallest of them only, and a group whose
    /// blocks were all in a larger group goes inside it.
    fn nest_overlapping_groups(&mut self) {
        let mut counts: HashMap<NodeId, usize> = HashMap::new();
        for group in &self.groups {
            for node in &group.nodes {
                *counts.entry(*node).or_default() += 1;
            }
        }
        if counts.values().all(|&count| count <= 1) {
            return;
        }
        let sets: Vec<(u64, BTreeSet<NodeId>)> = self.groups.iter().map(|g| (g.id, g.nodes.clone())).collect();
        for group in self.groups.iter_mut().filter(|g| g.parent.is_none()) {
            let mine = &group.nodes;
            group.parent = sets
                .iter()
                .filter(|(id, set)| {
                    *id != group.id
                        && mine.is_subset(set)
                        && (set.len() > mine.len() || (set.len() == mine.len() && *id < group.id))
                })
                .min_by_key(|(id, set)| (set.len(), std::cmp::Reverse(*id)))
                .map(|(id, _)| *id);
        }
        for (node, _) in counts.into_iter().filter(|(_, count)| *count > 1) {
            let keep = sets
                .iter()
                .filter(|(_, set)| set.contains(&node))
                .min_by_key(|(id, set)| (set.len(), std::cmp::Reverse(*id)))
                .map(|(id, _)| *id);
            for group in self.groups.iter_mut().filter(|g| Some(g.id) != keep) {
                group.nodes.remove(&node);
            }
        }
    }

    /// The input a flow output leads to.
    pub fn flow_target(&self, node: NodeId, port: &str) -> Option<&PortRef> {
        self.wires.iter().find(|w| w.from.node == node && w.from.port == port).map(|w| &w.to)
    }

    /// The output feeding a data input.
    pub fn data_source(&self, node: NodeId, port: &str) -> Option<&PortRef> {
        self.wires.iter().find(|w| w.to.node == node && w.to.port == port).map(|w| &w.from)
    }

    pub fn is_connected(&self, port: &PortRef) -> bool {
        self.wires.iter().any(|w| w.from == *port || w.to == *port)
    }

    /// Checks whether `from -> to` would be a legal wire, without changing anything.
    pub fn can_connect(&self, registry: &Registry, from: &PortRef, to: &PortRef) -> Result<PortKind> {
        let source = self.node(from.node).ok_or_else(|| Error::graph("the source block no longer exists"))?;
        let target = self.node(to.node).ok_or_else(|| Error::graph("the target block no longer exists"))?;
        if from.node == to.node {
            return Err(Error::graph("a block cannot be wired to itself"));
        }
        let out = registry
            .ports(source)
            .output(&from.port)
            .cloned()
            .ok_or_else(|| Error::graph("wires must start at an output"))?;
        let input = registry.ports(target).input(&to.port).ok_or_else(|| Error::graph("wires must end at an input"))?;
        match (out.kind, input.kind) {
            (PortKind::Flow, PortKind::Flow) => Ok(PortKind::Flow),
            (PortKind::Data(a), PortKind::Data(b)) if a.fits(b) => {
                if self.depends_on(from.node, to.node) {
                    return Err(Error::graph("that would make a value depend on itself"));
                }
                Ok(PortKind::Data(b))
            }
            (PortKind::Data(a), PortKind::Data(b)) => {
                Err(Error::graph(format!("“{}” cannot feed “{}”", a.label(), b.label())))
            }
            (PortKind::Flow, PortKind::Data(_)) => Err(Error::graph("a flow output cannot feed a value input")),
            (PortKind::Data(_), PortKind::Flow) => Err(Error::graph("a value output cannot feed a flow input")),
        }
    }

    /// Whether `node`'s values are computed (directly or indirectly) from `other`'s.
    fn depends_on(&self, node: NodeId, other: NodeId) -> bool {
        let mut stack = vec![node];
        let mut seen = BTreeSet::new();
        while let Some(current) = stack.pop() {
            if current == other {
                return true;
            }
            if !seen.insert(current) {
                continue;
            }
            // Only data wires end at ports other than the flow input.
            stack.extend(
                self.wires
                    .iter()
                    .filter(|w| w.to.node == current && w.to.port != crate::nodes::ports::FLOW_IN)
                    .map(|w| w.from.node),
            );
        }
        false
    }

    /// Adds a wire, replacing whichever existing wire it supersedes.
    pub fn connect(&mut self, registry: &Registry, from: PortRef, to: PortRef) -> Result<()> {
        match self.can_connect(registry, &from, &to)? {
            // A flow output leads to one place.
            PortKind::Flow => self.wires.retain(|w| w.from != from),
            // A data input has one source.
            PortKind::Data(_) => self.wires.retain(|w| w.to != to),
        }
        let wire = Wire { from, to };
        if !self.wires.contains(&wire) {
            self.wires.push(wire);
        }
        Ok(())
    }

    pub fn disconnect(&mut self, wire: &Wire) -> bool {
        let before = self.wires.len();
        self.wires.retain(|w| w != wire);
        self.wires.len() != before
    }

    /// Removes every wire attached to a port. Returns how many were removed.
    pub fn disconnect_port(&mut self, port: &PortRef) -> usize {
        let before = self.wires.len();
        self.wires.retain(|w| w.from != *port && w.to != *port);
        before - self.wires.len()
    }

    /// Removes wires whose ports no longer exist, e.g. after a Decision case
    /// was deleted. Nodes of unknown types are left alone: their ports cannot
    /// be known, and their wires must survive until the add-on is back.
    pub fn prune(&mut self, registry: &Registry) -> usize {
        let before = self.wires.len();
        let ports: HashMap<NodeId, _> = self
            .nodes
            .values()
            .filter(|n| registry.get(&n.kind).is_some())
            .map(|n| (n.id, registry.ports(n)))
            .collect();
        self.wires.retain(|w| {
            ports.get(&w.from.node).is_none_or(|p| p.output(&w.from.port).is_some())
                && ports.get(&w.to.node).is_none_or(|p| p.input(&w.to.port).is_some())
        });
        before - self.wires.len()
    }

    /// Copies the given nodes and the wires between them.
    pub fn fragment(&self, ids: &BTreeSet<NodeId>) -> Fragment {
        // The groups wholly inside the copy; one around them that is not stays behind.
        let groups: Vec<Group> = self
            .groups
            .iter()
            .filter(|g| {
                let blocks = self.group_blocks(g.id);
                !blocks.is_empty() && blocks.is_subset(ids)
            })
            .cloned()
            .collect();
        let kept: BTreeSet<u64> = groups.iter().map(|g| g.id).collect();
        let groups = groups
            .into_iter()
            .map(|mut g| {
                g.parent = g.parent.filter(|p| kept.contains(p));
                g
            })
            .collect();
        Fragment {
            groups,
            nodes: ids.iter().filter_map(|id| self.node(*id).cloned()).collect(),
            wires: self
                .wires
                .iter()
                .filter(|w| ids.contains(&w.from.node) && ids.contains(&w.to.node))
                .cloned()
                .collect(),
        }
    }

    /// Inserts a fragment, giving its nodes fresh ids and moving it so that its
    /// top-left corner lands on `at`. Returns the new ids.
    pub fn insert_fragment(&mut self, fragment: &Fragment, at: [f32; 2]) -> Vec<NodeId> {
        let origin = fragment.origin();
        let mut remap = HashMap::new();
        for node in &fragment.nodes {
            let id = self.fresh_id();
            remap.insert(node.id, id);
            let pos = [node.pos[0] - origin[0] + at[0], node.pos[1] - origin[1] + at[1]];
            self.nodes.insert(id, Node { id, pos, ..node.clone() });
        }
        for wire in &fragment.wires {
            if let (Some(&from), Some(&to)) = (remap.get(&wire.from.node), remap.get(&wire.to.node)) {
                self.wires.push(Wire {
                    from: PortRef::new(from, wire.from.port.clone()),
                    to: PortRef::new(to, wire.to.port.clone()),
                });
            }
        }
        // The fragment's groups get fresh ids, and keep their nesting.
        let first = self.groups.iter().map(|g| g.id + 1).max().unwrap_or(1);
        let group_ids: HashMap<u64, u64> =
            fragment.groups.iter().enumerate().map(|(i, g)| (g.id, first + i as u64)).collect();
        for group in &fragment.groups {
            let nodes: BTreeSet<NodeId> = group.nodes.iter().filter_map(|id| remap.get(id).copied()).collect();
            for existing in &mut self.groups {
                existing.nodes.retain(|n| !nodes.contains(n));
            }
            self.groups.push(Group {
                id: group_ids[&group.id],
                nodes,
                parent: group.parent.and_then(|p| group_ids.get(&p).copied()),
                ..group.clone()
            });
        }
        self.prune_groups();
        // A Stop block that continues from a copied block continues from the copy.
        for &id in remap.values() {
            if let Some(node) = self.nodes.get_mut(&id)
                && node.kind == ids::STOP
                && let Some(target) = node.params.get("target").and_then(Value::as_u64)
                && let Some(&new) = remap.get(&target)
            {
                node.params.insert("target".into(), Value::from(new));
            }
        }
        fragment.nodes.iter().map(|n| remap[&n.id]).collect()
    }

    /// Ids of every asset referenced by a node parameter.
    pub fn referenced_assets(&self, registry: &Registry) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for node in self.nodes() {
            if let Some(def) = registry.get(&node.kind) {
                out.extend(def.asset_ids(node));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::ports;

    fn registry() -> Registry {
        Registry::builtin()
    }

    #[test]
    fn groups_nest_and_blocks_leave_them_one_level_at_a_time() {
        let r = registry();
        let mut g = Graph::new();
        let [a, b, c] = [0.0, 1.0, 2.0].map(|x| g.add(&r, ids::DELAY, [x * 200.0, 0.0]).unwrap());
        let inner = g.add_group("inner", &BTreeSet::from([a, b])).unwrap();
        // Grouping the inner group with another block puts it inside the new one.
        let outer = g.add_group_around("outer", &BTreeSet::from([c]), &BTreeSet::from([inner])).unwrap();
        assert_eq!(g.group(inner).unwrap().parent, Some(outer));
        assert_eq!(g.group_of(a), Some(inner), "a block is directly in its innermost group only");
        assert_eq!(g.group_blocks(outer), BTreeSet::from([a, b, c]));
        assert_eq!(g.group_depth(inner), 1);

        // A group that is off turns off the groups inside it.
        g.group_mut(outer).unwrap().enabled = false;
        assert!(!g.is_active(a) && !g.with_groups_applied().node(a).unwrap().enabled);
        g.group_mut(outer).unwrap().enabled = true;

        // Taking a block out moves it one level out, not out of everything.
        g.take_out_of_group(a);
        assert_eq!(g.group_of(a), Some(outer));
        // Moving it back in.
        assert!(g.move_into_group(inner, &BTreeSet::from([a]), &BTreeSet::new()));
        assert_eq!(g.group_of(a), Some(inner));
        // A group cannot go inside a group inside itself.
        assert!(!g.move_into_group(inner, &BTreeSet::new(), &BTreeSet::from([outer])));

        // Ungrouping the outer group keeps the inner one, now at the top.
        g.remove_group(outer);
        assert_eq!(g.group(inner).unwrap().parent, None);
        assert_eq!(g.group_of(c), None);
        // A group left with nothing in it goes away.
        g.take_out_of_group(a);
        g.take_out_of_group(b);
        assert!(g.groups().is_empty());
    }

    #[test]
    fn blocks_grouped_twice_in_old_files_become_nested_groups() {
        let r = registry();
        let mut g = Graph::new();
        let [a, b, c] = [0.0, 1.0, 2.0].map(|x| g.add(&r, ids::DELAY, [x * 200.0, 0.0]).unwrap());
        let mut file = Fragment::from(g);
        let group = |id, nodes: &[NodeId]| Group {
            id,
            name: format!("g{id}"),
            color: None,
            nodes: nodes.iter().copied().collect(),
            parent: None,
            enabled: true,
        };
        // Grouped twice the same, and once with one more block.
        file.groups = vec![group(1, &[a, b, c]), group(2, &[a, b]), group(3, &[a, b])];
        let g = Graph::from(file);
        let parent = |id| g.group(id).unwrap().parent;
        assert_eq!((parent(1), parent(2), parent(3)), (None, Some(1), Some(2)));
        assert_eq!(g.group_of(a), Some(3));
        assert_eq!(g.group_of(c), Some(1));

        // A copy of everything keeps the nesting, with fresh ids.
        let copy = g.fragment(&BTreeSet::from([a, b, c]));
        assert_eq!(copy.groups.len(), 3);
        let mut other = Graph::new();
        let ids = other.insert_fragment(&copy, [0.0, 0.0]);
        let innermost = other.group_of(ids[0]).unwrap();
        assert_eq!(other.group_depth(innermost), 2);
    }

    #[test]
    fn a_flow_output_leads_to_one_place() {
        let r = registry();
        let mut g = Graph::new();
        let start = g.add(&r, ids::START, [0.0, 0.0]).unwrap();
        let a = g.add(&r, ids::DELAY, [0.0, 100.0]).unwrap();
        let b = g.add(&r, ids::DELAY, [200.0, 100.0]).unwrap();

        g.connect(&r, PortRef::new(start, ports::OUT), PortRef::new(a, ports::FLOW_IN)).unwrap();
        g.connect(&r, PortRef::new(start, ports::OUT), PortRef::new(b, ports::FLOW_IN)).unwrap();
        assert_eq!(g.wires().len(), 1);
        assert_eq!(g.flow_target(start, ports::OUT).unwrap().node, b);
    }

    #[test]
    fn a_flow_input_accepts_many_wires() {
        let r = registry();
        let mut g = Graph::new();
        let a = g.add(&r, ids::DELAY, [0.0, 0.0]).unwrap();
        let b = g.add(&r, ids::DELAY, [0.0, 0.0]).unwrap();
        let c = g.add(&r, ids::DELAY, [0.0, 0.0]).unwrap();
        g.connect(&r, PortRef::new(a, ports::OUT), PortRef::new(c, ports::FLOW_IN)).unwrap();
        g.connect(&r, PortRef::new(b, ports::OUT), PortRef::new(c, ports::FLOW_IN)).unwrap();
        assert_eq!(g.wires().len(), 2);
    }

    #[test]
    fn mismatched_ports_are_rejected() {
        let r = registry();
        let mut g = Graph::new();
        let start = g.add(&r, ids::START, [0.0, 0.0]).unwrap();
        let find = g.add(&r, ids::FIND_IMAGE, [0.0, 0.0]).unwrap();
        let decision = g.add(&r, ids::DECISION, [0.0, 0.0]).unwrap();
        let input = g.add(&r, ids::INPUT, [0.0, 0.0]).unwrap();

        // flow -> data
        assert!(g.connect(&r, PortRef::new(start, ports::OUT), PortRef::new(decision, "cond:1")).is_err());
        // bool -> point
        assert!(g.connect(&r, PortRef::new(find, "found"), PortRef::new(input, "position")).is_err());
        // input used as a source
        assert!(g.connect(&r, PortRef::new(decision, "cond:1"), PortRef::new(input, "position")).is_err());
        // legal
        g.connect(&r, PortRef::new(find, "found"), PortRef::new(decision, "cond:1")).unwrap();
        g.connect(&r, PortRef::new(find, "position"), PortRef::new(input, "position")).unwrap();
    }

    #[test]
    fn value_cycles_are_rejected() {
        let r = registry();
        let mut g = Graph::new();
        let a = g.add(&r, ids::GATE, [0.0, 0.0]).unwrap();
        let b = g.add(&r, ids::GATE, [0.0, 0.0]).unwrap();
        g.connect(&r, PortRef::new(a, "out"), PortRef::new(b, "a")).unwrap();
        assert!(g.connect(&r, PortRef::new(b, "out"), PortRef::new(a, "a")).is_err());
    }

    #[test]
    fn removing_a_case_prunes_its_wires() {
        let r = registry();
        let mut g = Graph::new();
        let find = g.add(&r, ids::FIND_IMAGE, [0.0, 0.0]).unwrap();
        let decision = g.add(&r, ids::DECISION, [0.0, 0.0]).unwrap();
        g.connect(&r, PortRef::new(find, "found"), PortRef::new(decision, "cond:1")).unwrap();

        g.node_mut(decision).unwrap().set_param("cases", serde_json::json!([]));
        assert_eq!(g.prune(&r), 1);
        assert!(g.wires().is_empty());
    }

    #[test]
    fn fragments_paste_with_fresh_ids_and_internal_wires() {
        let r = registry();
        let mut g = Graph::new();
        let start = g.add(&r, ids::START, [10.0, 10.0]).unwrap();
        let wait = g.add(&r, ids::DELAY, [10.0, 110.0]).unwrap();
        let other = g.add(&r, ids::DELAY, [300.0, 0.0]).unwrap();
        g.connect(&r, PortRef::new(start, ports::OUT), PortRef::new(wait, ports::FLOW_IN)).unwrap();
        g.connect(&r, PortRef::new(wait, ports::OUT), PortRef::new(other, ports::FLOW_IN)).unwrap();

        let fragment = g.fragment(&BTreeSet::from([start, wait]));
        assert_eq!(fragment.wires.len(), 1, "the wire leaving the selection is not copied");

        let pasted = g.insert_fragment(&fragment, [500.0, 500.0]);
        assert_eq!(pasted.len(), 2);
        assert!(pasted.iter().all(|id| ![start, wait, other].contains(id)));
        assert_eq!(g.node(pasted[0]).unwrap().pos, [500.0, 500.0]);
        assert_eq!(g.node(pasted[1]).unwrap().pos, [500.0, 600.0]);
        assert_eq!(g.flow_target(pasted[0], ports::OUT).unwrap().node, pasted[1]);
    }

    #[test]
    fn graphs_round_trip_through_json() {
        let r = registry();
        let mut g = Graph::new();
        let start = g.add(&r, ids::START, [0.0, 0.0]).unwrap();
        let wait = g.add(&r, ids::DELAY, [0.0, 100.0]).unwrap();
        g.connect(&r, PortRef::new(start, ports::OUT), PortRef::new(wait, ports::FLOW_IN)).unwrap();

        let json = serde_json::to_string_pretty(&g).unwrap();
        let back: Graph = serde_json::from_str(&json).unwrap();
        assert_eq!(back, g);
        assert!(json.contains(r#""type": "flow.start""#));

        // Ids continue after the highest loaded id.
        let mut back = back;
        assert_eq!(back.add(&r, ids::DELAY, [0.0, 0.0]).unwrap(), 3);
    }
}
