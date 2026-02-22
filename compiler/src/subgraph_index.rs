use std::collections::{HashMap, HashSet};

use crate::graph::{Edge, Node, NodeId, NodeKind, ProgramGraph, Subgraph, TaskGraph};

#[derive(Debug, Clone, Default)]
pub struct SubgraphIndex {
    node_pos: HashMap<NodeId, usize>,
    first_incoming_edge_pos: HashMap<NodeId, usize>,
    first_outgoing_edge_pos: HashMap<NodeId, usize>,
    incoming_edge_count: HashMap<NodeId, usize>,
    outgoing_edge_count: HashMap<NodeId, usize>,
    edge_exists: HashSet<(NodeId, NodeId)>,
}

const INDEX_MIN_GRAPH_SIZE: usize = 32;

impl SubgraphIndex {
    pub fn build(sub: &Subgraph) -> Self {
        let mut index = SubgraphIndex::default();
        for (i, node) in sub.nodes.iter().enumerate() {
            index.node_pos.insert(node.id, i);
        }
        for (i, edge) in sub.edges.iter().enumerate() {
            index
                .first_incoming_edge_pos
                .entry(edge.target)
                .or_insert(i);
            index
                .first_outgoing_edge_pos
                .entry(edge.source)
                .or_insert(i);
            *index.incoming_edge_count.entry(edge.target).or_insert(0) += 1;
            *index.outgoing_edge_count.entry(edge.source).or_insert(0) += 1;
            index.edge_exists.insert((edge.source, edge.target));
        }
        index
    }

    pub fn node<'a>(&self, sub: &'a Subgraph, id: NodeId) -> Option<&'a Node> {
        self.node_pos.get(&id).and_then(|&i| sub.nodes.get(i))
    }

    pub fn first_incoming_edge<'a>(&self, sub: &'a Subgraph, id: NodeId) -> Option<&'a Edge> {
        self.first_incoming_edge_pos
            .get(&id)
            .and_then(|&i| sub.edges.get(i))
    }

    pub fn first_outgoing_edge<'a>(&self, sub: &'a Subgraph, id: NodeId) -> Option<&'a Edge> {
        self.first_outgoing_edge_pos
            .get(&id)
            .and_then(|&i| sub.edges.get(i))
    }

    pub fn incoming_count(&self, id: NodeId) -> usize {
        self.incoming_edge_count.get(&id).copied().unwrap_or(0)
    }

    pub fn outgoing_count(&self, id: NodeId) -> usize {
        self.outgoing_edge_count.get(&id).copied().unwrap_or(0)
    }

    pub fn has_edge(&self, src: NodeId, tgt: NodeId) -> bool {
        self.edge_exists.contains(&(src, tgt))
    }
}

pub fn subgraph_key(sub: &Subgraph) -> usize {
    sub as *const Subgraph as usize
}

pub fn build_subgraph_indices(graph: &ProgramGraph) -> HashMap<usize, SubgraphIndex> {
    let mut indices = HashMap::new();
    for_each_subgraph(graph, |sub| {
        if should_index(sub) {
            indices.insert(subgraph_key(sub), SubgraphIndex::build(sub));
        }
    });
    indices
}

pub fn build_global_node_index(
    graph: &ProgramGraph,
    indexed_subgraphs: &HashMap<usize, SubgraphIndex>,
) -> HashMap<NodeId, (usize, usize)> {
    let mut nodes = HashMap::new();
    if indexed_subgraphs.is_empty() {
        return nodes;
    }
    for_each_subgraph(graph, |sub| {
        let key = subgraph_key(sub);
        if !indexed_subgraphs.contains_key(&key) {
            return;
        }
        for (node_pos, node) in sub.nodes.iter().enumerate() {
            nodes.insert(node.id, (key, node_pos));
        }
    });
    nodes
}

fn should_index(sub: &Subgraph) -> bool {
    sub.nodes.len() + sub.edges.len() >= INDEX_MIN_GRAPH_SIZE
}

fn for_each_subgraph<F>(graph: &ProgramGraph, mut f: F)
where
    F: FnMut(&Subgraph),
{
    for task_graph in graph.tasks.values() {
        for sub in subgraphs_of(task_graph) {
            f(sub);
        }
    }
}

// ── Shared graph query helpers ─────────────────────────────────────────────
//
// These replace duplicated free functions that existed in analyze.rs,
// schedule.rs, and codegen.rs.

/// Find a node in a subgraph by NodeId (linear scan).
pub fn find_node(sub: &Subgraph, id: NodeId) -> Option<&Node> {
    sub.nodes.iter().find(|n| n.id == id)
}

/// Get all subgraphs from a TaskGraph.
pub fn subgraphs_of(task_graph: &TaskGraph) -> Vec<&Subgraph> {
    match task_graph {
        TaskGraph::Pipeline(sub) => vec![sub],
        TaskGraph::Modal { control, modes } => {
            let mut subs = vec![control];
            for (_, sub) in modes {
                subs.push(sub);
            }
            subs
        }
    }
}

/// Identify feedback back-edges in a subgraph.
///
/// For each detected cycle, the outgoing edge from the `delay` actor is
/// treated as the back-edge (delay provides initial tokens, breaking the
/// data-flow dependency for topological sorting).
pub fn identify_back_edges(sub: &Subgraph, cycles: &[Vec<NodeId>]) -> HashSet<(NodeId, NodeId)> {
    let mut back_edges = HashSet::new();
    let node_ids: HashSet<u32> = sub.nodes.iter().map(|n| n.id.0).collect();

    for cycle in cycles {
        if !cycle.iter().all(|id| node_ids.contains(&id.0)) {
            continue;
        }
        for (i, &nid) in cycle.iter().enumerate() {
            if let Some(node) = find_node(sub, nid) {
                if matches!(&node.kind, NodeKind::Actor { name, .. } if name == "delay") {
                    let next_nid = cycle[(i + 1) % cycle.len()];
                    if sub
                        .edges
                        .iter()
                        .any(|e| e.source == nid && e.target == next_nid)
                    {
                        back_edges.insert((nid, next_nid));
                    }
                    break;
                }
            }
        }
    }
    back_edges
}

/// Query context for efficient graph lookups, with optional index fallback.
pub struct GraphQueryCtx<'a> {
    subgraph_indices: &'a HashMap<usize, SubgraphIndex>,
}

impl<'a> GraphQueryCtx<'a> {
    pub fn new(subgraph_indices: &'a HashMap<usize, SubgraphIndex>) -> Self {
        Self { subgraph_indices }
    }

    fn subgraph_index(&self, sub: &Subgraph) -> Option<&SubgraphIndex> {
        self.subgraph_indices.get(&subgraph_key(sub))
    }

    pub fn node_in_subgraph<'s>(&self, sub: &'s Subgraph, id: NodeId) -> Option<&'s Node> {
        self.subgraph_index(sub)
            .and_then(|idx| idx.node(sub, id))
            .or_else(|| find_node(sub, id))
    }

    pub fn first_incoming_edge<'s>(&self, sub: &'s Subgraph, id: NodeId) -> Option<&'s Edge> {
        self.subgraph_index(sub)
            .and_then(|idx| idx.first_incoming_edge(sub, id))
            .or_else(|| sub.edges.iter().find(|e| e.target == id))
    }

    pub fn first_outgoing_edge<'s>(&self, sub: &'s Subgraph, id: NodeId) -> Option<&'s Edge> {
        self.subgraph_index(sub)
            .and_then(|idx| idx.first_outgoing_edge(sub, id))
            .or_else(|| sub.edges.iter().find(|e| e.source == id))
    }

    pub fn incoming_edge_count(&self, sub: &Subgraph, id: NodeId) -> usize {
        self.subgraph_index(sub)
            .map(|idx| idx.incoming_count(id))
            .unwrap_or_else(|| sub.edges.iter().filter(|e| e.target == id).count())
    }

    pub fn outgoing_edge_count(&self, sub: &Subgraph, id: NodeId) -> usize {
        self.subgraph_index(sub)
            .map(|idx| idx.outgoing_count(id))
            .unwrap_or_else(|| sub.edges.iter().filter(|e| e.source == id).count())
    }
}
