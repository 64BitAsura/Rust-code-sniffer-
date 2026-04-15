//! MCP tool definitions and dispatch.

use std::path::PathBuf;
use serde_json::Value;

use crate::graph::{AdjacencyStore, GraphStore};
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
    ]
}

pub fn call_tool(name: &str, params: Value, index_dir: &PathBuf) -> Result<String, String> {
    match name {
        "query" => tool_query(params, index_dir),
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

