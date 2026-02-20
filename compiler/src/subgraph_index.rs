use std::collections::HashMap;

use crate::graph::{Edge, Node, NodeId, ProgramGraph, Subgraph, TaskGraph};

#[derive(Debug, Clone, Default)]
pub struct SubgraphIndex {
    node_pos: HashMap<NodeId, usize>,
    first_incoming_edge_pos: HashMap<NodeId, usize>,
    first_outgoing_edge_pos: HashMap<NodeId, usize>,
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
        match task_graph {
            TaskGraph::Pipeline(sub) => f(sub),
            TaskGraph::Modal { control, modes } => {
                f(control);
                for (_, sub) in modes {
                    f(sub);
                }
            }
        }
    }
}
