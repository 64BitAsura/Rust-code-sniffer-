//! JSON-persisted adjacency-list graph store.
//!
//! Nodes and edges are kept in two separate `HashMap`s (keyed by their `id`
//! fields) and flushed to two compact JSON files:
//!
//! ```text
//! <index_dir>/graph/nodes.json
//! <index_dir>/graph/edges.json
//! ```
//!
//! The files contain flat JSON arrays for easy inspection and portability.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::SnifferError;

use super::{Edge, GraphStore, Node};

const GRAPH_SUBDIR: &str = "graph";
const NODES_FILE: &str = "nodes.json";
const EDGES_FILE: &str = "edges.json";

/// In-memory adjacency-list graph store backed by two JSON files.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AdjacencyStore {
    nodes: HashMap<String, Node>,
    edges: HashMap<String, Edge>,
}

impl GraphStore for AdjacencyStore {
    fn upsert_node(&mut self, node: Node) {
        self.nodes.insert(node.id.clone(), node);
    }

    fn upsert_edge(&mut self, edge: Edge) {
        self.edges.insert(edge.id.clone(), edge);
    }

    fn remove_by_file(&mut self, file_path: &str) -> usize {
        // Collect node IDs that belong to this file.
        let stale_node_ids: Vec<String> = self
            .nodes
            .values()
            .filter(|n| n.file_path == file_path || n.id == format!("file:{file_path}"))
            .map(|n| n.id.clone())
            .collect();

        // Remove edges that touch any of those nodes.
        let stale_count = stale_node_ids.len();
        if stale_count > 0 {
            let id_set: std::collections::HashSet<&str> =
                stale_node_ids.iter().map(|s| s.as_str()).collect();
            self.edges
                .retain(|_, e| !id_set.contains(e.source_id.as_str()) && !id_set.contains(e.target_id.as_str()));

            for id in &stale_node_ids {
                self.nodes.remove(id);
            }
        }

        stale_count
    }

    fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn edge_count(&self) -> usize {
        self.edges.len()
    }

    fn save(&self, index_dir: &Path) -> Result<(), SnifferError> {
        let graph_dir = index_dir.join(GRAPH_SUBDIR);
        fs::create_dir_all(&graph_dir)
            .map_err(|e| SnifferError::Io(format!("creating graph dir: {e}")))?;

        // Serialise as sorted arrays for deterministic output.
        let mut node_list: Vec<&Node> = self.nodes.values().collect();
        node_list.sort_by(|a, b| a.id.cmp(&b.id));
        let nodes_json = serde_json::to_string_pretty(&node_list)
            .map_err(|e| SnifferError::Json(format!("serialising nodes: {e}")))?;
        fs::write(graph_dir.join(NODES_FILE), nodes_json)
            .map_err(|e| SnifferError::Io(format!("writing nodes.json: {e}")))?;

        let mut edge_list: Vec<&Edge> = self.edges.values().collect();
        edge_list.sort_by(|a, b| a.id.cmp(&b.id));
        let edges_json = serde_json::to_string_pretty(&edge_list)
            .map_err(|e| SnifferError::Json(format!("serialising edges: {e}")))?;
        fs::write(graph_dir.join(EDGES_FILE), edges_json)
            .map_err(|e| SnifferError::Io(format!("writing edges.json: {e}")))?;

        Ok(())
    }

    fn load(index_dir: &Path) -> Result<Self, SnifferError> {
        let graph_dir = index_dir.join(GRAPH_SUBDIR);

        let nodes_path = graph_dir.join(NODES_FILE);
        let edges_path = graph_dir.join(EDGES_FILE);

        // If the graph directory doesn't exist yet, return an empty store.
        if !nodes_path.exists() && !edges_path.exists() {
            return Ok(Self::default());
        }

        let nodes: Vec<Node> = if nodes_path.exists() {
            let raw = fs::read_to_string(&nodes_path)
                .map_err(|e| SnifferError::Io(format!("reading nodes.json: {e}")))?;
            serde_json::from_str(&raw)
                .map_err(|e| SnifferError::Json(format!("parsing nodes.json: {e}")))?
        } else {
            Vec::new()
        };

        let edges: Vec<Edge> = if edges_path.exists() {
            let raw = fs::read_to_string(&edges_path)
                .map_err(|e| SnifferError::Io(format!("reading edges.json: {e}")))?;
            serde_json::from_str(&raw)
                .map_err(|e| SnifferError::Json(format!("parsing edges.json: {e}")))?
        } else {
            Vec::new()
        };

        Ok(Self {
            nodes: nodes.into_iter().map(|n| (n.id.clone(), n)).collect(),
            edges: edges.into_iter().map(|e| (e.id.clone(), e)).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EdgeType, NodeLabel};
    use tempfile::TempDir;

    fn make_file_node(path: &str) -> Node {
        Node {
            id: format!("file:{path}"),
            label: NodeLabel::File,
            name: path.to_string(),
            file_path: String::new(),
            start_line: 0,
            end_line: 0,
        }
    }

    fn make_fn_node(file: &str, name: &str, start: usize, end: usize) -> Node {
        Node {
            id: format!("fn:{file}::{name}"),
            label: NodeLabel::Function,
            name: name.to_string(),
            file_path: file.to_string(),
            start_line: start,
            end_line: end,
        }
    }

    fn make_edge(src: &str, tgt: &str, etype: EdgeType) -> Edge {
        Edge {
            id: format!("{src}--{etype}-->{tgt}"),
            source_id: src.to_string(),
            target_id: tgt.to_string(),
            edge_type: etype,
            confidence: 1.0,
            reason: String::new(),
        }
    }

    #[test]
    fn upsert_and_count() {
        let mut store = AdjacencyStore::default();
        store.upsert_node(make_file_node("src/main.rs"));
        store.upsert_node(make_fn_node("src/main.rs", "main", 1, 5));
        assert_eq!(store.node_count(), 2);

        store.upsert_edge(make_edge(
            "file:src/main.rs",
            "fn:src/main.rs::main",
            EdgeType::Defines,
        ));
        assert_eq!(store.edge_count(), 1);
    }

    #[test]
    fn upsert_is_idempotent() {
        let mut store = AdjacencyStore::default();
        store.upsert_node(make_file_node("src/a.rs"));
        store.upsert_node(make_file_node("src/a.rs")); // duplicate
        assert_eq!(store.node_count(), 1);
    }

    #[test]
    fn remove_by_file_purges_nodes_and_edges() {
        let mut store = AdjacencyStore::default();
        store.upsert_node(make_file_node("src/a.rs"));
        store.upsert_node(make_fn_node("src/a.rs", "foo", 1, 3));
        store.upsert_node(make_fn_node("src/b.rs", "bar", 1, 3));
        store.upsert_edge(make_edge(
            "fn:src/a.rs::foo",
            "fn:src/b.rs::bar",
            EdgeType::Calls,
        ));

        let removed = store.remove_by_file("src/a.rs");
        // The File node and the Function node from a.rs should be gone.
        assert_eq!(removed, 2);
        // The edge that referenced a.rs::foo should also be gone.
        assert_eq!(store.edge_count(), 0);
        // b.rs::bar should still be there.
        assert_eq!(store.node_count(), 1);
    }

    #[test]
    fn round_trip_persistence() {
        let dir = TempDir::new().unwrap();

        let mut store = AdjacencyStore::default();
        store.upsert_node(make_file_node("src/lib.rs"));
        store.upsert_node(make_fn_node("src/lib.rs", "helper", 10, 20));
        store.upsert_edge(make_edge(
            "file:src/lib.rs",
            "fn:src/lib.rs::helper",
            EdgeType::Defines,
        ));
        store.save(dir.path()).unwrap();

        let loaded = AdjacencyStore::load(dir.path()).unwrap();
        assert_eq!(loaded.node_count(), 2);
        assert_eq!(loaded.edge_count(), 1);
    }

    #[test]
    fn load_returns_empty_when_no_graph_dir() {
        let dir = TempDir::new().unwrap();
        let store = AdjacencyStore::load(dir.path()).unwrap();
        assert_eq!(store.node_count(), 0);
        assert_eq!(store.edge_count(), 0);
    }
}
