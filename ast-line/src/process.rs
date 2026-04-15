//! Execution flow tracing — BFS from entry-point functions.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::SnifferError;
use crate::graph::{AdjacencyStore, EdgeType, GraphStore, NodeLabel};

const PROCESSES_FILE: &str = "processes.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Process {
    pub uid: String,
    pub name: String,
    pub entry_point_id: String,
    pub steps: Vec<String>,
    pub step_count: usize,
    pub communities: Vec<String>,
    pub heuristic_label: String,
}

pub fn trace_processes(graph: &AdjacencyStore, max_depth: usize) -> Vec<Process> {
    let mut calls_from: HashMap<String, Vec<String>> = HashMap::new();
    let mut in_degree: HashMap<String, usize> = HashMap::new();

    for node in graph.nodes() {
        if node.label == NodeLabel::Function {
            in_degree.entry(node.id.clone()).or_insert(0);
        }
    }

    for edge in graph.edges() {
        if edge.edge_type == EdgeType::Calls {
            calls_from
                .entry(edge.source_id.clone())
                .or_default()
                .push(edge.target_id.clone());
            *in_degree.entry(edge.target_id.clone()).or_insert(0) += 1;
        }
    }

    let entry_points: Vec<String> = graph
        .nodes()
        .filter(|n| n.label == NodeLabel::Function)
        .filter(|n| *in_degree.get(&n.id).unwrap_or(&0) == 0 || n.name == "main")
        .map(|n| n.id.clone())
        .collect();

    let node_names: HashMap<String, String> =
        graph.nodes().map(|n| (n.id.clone(), n.name.clone())).collect();

    entry_points
        .into_iter()
        .enumerate()
        .map(|(i, entry_id)| {
            let mut steps = Vec::new();
            let mut visited = HashSet::new();
            let mut queue = VecDeque::new();
            queue.push_back((entry_id.clone(), 0usize));

            while let Some((id, depth)) = queue.pop_front() {
                if depth > max_depth || visited.contains(&id) {
                    continue;
                }
                visited.insert(id.clone());
                steps.push(id.clone());
                if let Some(callees) = calls_from.get(&id) {
                    for c in callees {
                        if !visited.contains(c) {
                            queue.push_back((c.clone(), depth + 1));
                        }
                    }
                }
            }

            let name = node_names
                .get(&entry_id)
                .cloned()
                .unwrap_or_else(|| format!("process_{i}"));
            let step_count = steps.len();
            Process {
                uid: format!("process_{i}"),
                name: name.clone(),
                entry_point_id: entry_id,
                steps,
                step_count,
                communities: vec![],
                heuristic_label: name,
            }
        })
        .collect()
}

pub fn save_processes(index_dir: &Path, processes: &[Process]) -> Result<(), SnifferError> {
    fs::create_dir_all(index_dir)
        .map_err(|e| SnifferError::Io(format!("creating index dir: {e}")))?;
    let json = serde_json::to_string_pretty(processes)
        .map_err(|e| SnifferError::Json(format!("serialising processes: {e}")))?;
    fs::write(index_dir.join(PROCESSES_FILE), json)
        .map_err(|e| SnifferError::Io(format!("writing processes.json: {e}")))?;
    Ok(())
}

pub fn load_processes(index_dir: &Path) -> Vec<Process> {
    let path = index_dir.join(PROCESSES_FILE);
    let raw = match fs::read_to_string(path) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    serde_json::from_str(&raw).unwrap_or_default()
}
