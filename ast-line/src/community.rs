//! Community detection via connected-components on the Calls/Imports graph.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::SnifferError;
use crate::graph::{AdjacencyStore, EdgeType, GraphStore};

const COMMUNITIES_FILE: &str = "communities.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Community {
    pub uid: String,
    pub symbol_ids: Vec<String>,
    pub cohesion: f64,
    pub heuristic_label: String,
    pub keywords: Vec<String>,
}

fn uf_find(parent: &mut Vec<usize>, mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn uf_union(parent: &mut Vec<usize>, x: usize, y: usize) {
    let rx = uf_find(parent, x);
    let ry = uf_find(parent, y);
    if rx != ry {
        parent[rx] = ry;
    }
}

pub fn detect_communities(graph: &AdjacencyStore) -> Vec<Community> {
    let node_ids: Vec<String> = graph.nodes().map(|n| n.id.clone()).collect();
    if node_ids.is_empty() {
        return vec![];
    }

    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for edge in graph.edges() {
        if matches!(edge.edge_type, EdgeType::Calls | EdgeType::Imports) {
            adj.entry(edge.source_id.clone()).or_default().push(edge.target_id.clone());
            adj.entry(edge.target_id.clone()).or_default().push(edge.source_id.clone());
        }
    }

    let id_to_idx: HashMap<&str, usize> =
        node_ids.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();
    let mut parent: Vec<usize> = (0..node_ids.len()).collect();

    for (src, dsts) in &adj {
        if let Some(&si) = id_to_idx.get(src.as_str()) {
            for dst in dsts {
                if let Some(&di) = id_to_idx.get(dst.as_str()) {
                    uf_union(&mut parent, si, di);
                }
            }
        }
    }

    let mut groups: HashMap<usize, Vec<String>> = HashMap::new();
    for (i, id) in node_ids.iter().enumerate() {
        let root = uf_find(&mut parent, i);
        groups.entry(root).or_default().push(id.clone());
    }

    let total_edges = graph.edge_count();

    groups
        .into_values()
        .enumerate()
        .map(|(i, members)| {
            let member_set: std::collections::HashSet<&str> =
                members.iter().map(|s| s.as_str()).collect();
            let intra = graph
                .edges()
                .filter(|e| {
                    member_set.contains(e.source_id.as_str())
                        && member_set.contains(e.target_id.as_str())
                })
                .count();
            let cohesion = if total_edges > 0 {
                intra as f64 / total_edges as f64
            } else {
                0.0
            };

            let mut word_freq: HashMap<String, usize> = HashMap::new();
            for id in &members {
                for part in id.split("::").last().unwrap_or("").split('_') {
                    if part.len() > 2 {
                        *word_freq.entry(part.to_lowercase()).or_default() += 1;
                    }
                }
            }
            let mut words: Vec<(String, usize)> = word_freq.into_iter().collect();
            words.sort_by(|a, b| b.1.cmp(&a.1));
            let heuristic_label = words
                .first()
                .map(|(w, _)| w.clone())
                .unwrap_or_else(|| format!("cluster_{i}"));
            let keywords: Vec<String> =
                words.iter().take(3).map(|(w, _)| w.clone()).collect();

            Community {
                uid: format!("community_{i}"),
                symbol_ids: members,
                cohesion,
                heuristic_label,
                keywords,
            }
        })
        .collect()
}

pub fn save_communities(index_dir: &Path, communities: &[Community]) -> Result<(), SnifferError> {
    fs::create_dir_all(index_dir)
        .map_err(|e| SnifferError::Io(format!("creating index dir: {e}")))?;
    let json = serde_json::to_string_pretty(communities)
        .map_err(|e| SnifferError::Json(format!("serialising communities: {e}")))?;
    fs::write(index_dir.join(COMMUNITIES_FILE), json)
        .map_err(|e| SnifferError::Io(format!("writing communities.json: {e}")))?;
    Ok(())
}

pub fn load_communities(index_dir: &Path) -> Vec<Community> {
    let path = index_dir.join(COMMUNITIES_FILE);
    let raw = match fs::read_to_string(path) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    serde_json::from_str(&raw).unwrap_or_default()
}
