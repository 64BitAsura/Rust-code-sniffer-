//! MCP tool definitions and dispatch.

use std::path::PathBuf;
use serde_json::Value;

use crate::graph::{AdjacencyStore, EdgeType, GraphStore};
use crate::search::{BM25Index, hybrid_search};

pub fn list_tools() -> Vec<Value> {
    vec![
        serde_json::json!({
            "name": "query",
            "description": "Hybrid search returning execution flows matching the query.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "limit": { "type": "integer", "description": "Max results", "default": 10 }
                },
                "required": ["query"]
            }
        }),
        serde_json::json!({
            "name": "context",
            "description": "360-degree view of a single symbol: callers, callees, execution flows.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Symbol name to look up" }
                },
                "required": ["name"]
            }
        }),
    ]
}

pub fn call_tool(name: &str, params: Value, index_dir: &PathBuf) -> Result<String, String> {
    match name {
        "query" => tool_query(params, index_dir),
        "context" => tool_context(params, index_dir),
        _ => Err(format!("Unknown tool: {name}")),
    }
}

pub fn read_resource(uri: &str, index_dir: &PathBuf) -> String {
    match uri {
        "gitnexus://repo/context" => {
            let graph = AdjacencyStore::load(index_dir).unwrap_or_default();
            let meta = crate::meta::IndexMeta::load(index_dir);
            serde_json::json!({
                "node_count": graph.node_count(),
                "edge_count": graph.edge_count(),
                "indexed_at": meta.as_ref().map(|m| m.indexed_at.clone()),
                "root": meta.as_ref().map(|m| m.root.clone()),
            }).to_string()
        }
        "gitnexus://repo/schema" => {
            serde_json::json!({
                "node_labels": ["File","Function","Struct","Enum","Trait","Impl","Module",
                    "TypeAlias","Constant","Static","Macro","Field","Community","Process","Route"],
                "edge_types": ["CALLS","IMPORTS","EXTENDS","IMPLEMENTS","HAS_METHOD",
                    "HAS_PROPERTY","ACCESSES","METHOD_OVERRIDES","METHOD_IMPLEMENTS",
                    "CONTAINS","DEFINES","MEMBER_OF","STEP_IN_PROCESS","HANDLES_ROUTE"]
            }).to_string()
        }
        _ => "{}".to_owned(),
    }
}

fn load_graph(index_dir: &PathBuf) -> Result<AdjacencyStore, String> {
    AdjacencyStore::load(index_dir).map_err(|e| format!("Failed to load graph: {e}"))
}

fn tool_query(params: Value, index_dir: &PathBuf) -> Result<String, String> {
    let query = params["query"].as_str().unwrap_or("").to_owned();
    let limit = params["limit"].as_u64().unwrap_or(10) as usize;

    let graph = load_graph(index_dir)?;
    let mut bm25 = BM25Index::new();
    for node in graph.nodes() {
        bm25.add_document(node.id.clone(), &node.name, &node.file_path);
    }
    bm25.build();

    let results = hybrid_search(&bm25, &query, &[], limit);
    let processes = crate::process::load_processes(index_dir);

    let output = serde_json::json!({
        "query": query,
        "results": results.iter().map(|r| serde_json::json!({
            "symbol_id": r.symbol_id,
            "name": r.name,
            "file_path": r.file_path,
            "score": r.score,
        })).collect::<Vec<_>>(),
        "related_processes": processes.iter()
            .filter(|p| results.iter().any(|r| p.steps.contains(&r.symbol_id)))
            .take(5)
            .map(|p| serde_json::json!({ "uid": p.uid, "name": p.name, "step_count": p.step_count }))
            .collect::<Vec<_>>()
    });
    Ok(output.to_string())
}

fn tool_context(params: Value, index_dir: &PathBuf) -> Result<String, String> {
    let name = params["name"].as_str().unwrap_or("").to_owned();
    let graph = load_graph(index_dir)?;

    let matching_nodes: Vec<_> = graph.nodes()
        .filter(|n| n.name == name)
        .map(|n| serde_json::json!({
            "id": n.id, "label": n.label.to_string(),
            "file_path": n.file_path, "start_line": n.start_line
        }))
        .collect();

    let all_edges: Vec<_> = graph.edges().collect();
    let callers: Vec<_> = matching_nodes.iter().flat_map(|n| {
        let node_id = n["id"].as_str().unwrap_or("");
        all_edges.iter()
            .filter(|e| e.target_id == node_id && e.edge_type == EdgeType::Calls)
            .map(|e| e.source_id.clone())
            .collect::<Vec<_>>()
    }).collect();

    let callees: Vec<_> = matching_nodes.iter().flat_map(|n| {
        let node_id = n["id"].as_str().unwrap_or("");
        all_edges.iter()
            .filter(|e| e.source_id == node_id && e.edge_type == EdgeType::Calls)
            .map(|e| e.target_id.clone())
            .collect::<Vec<_>>()
    }).collect();

    Ok(serde_json::json!({
        "name": name,
        "nodes": matching_nodes,
        "callers": callers,
        "callees": callees,
    }).to_string())
}

