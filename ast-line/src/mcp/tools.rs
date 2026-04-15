//! MCP tool definitions and dispatch.

use std::path::PathBuf;
use serde_json::Value;

use crate::graph::{AdjacencyStore, GraphStore};

pub fn list_tools() -> Vec<Value> {
    vec![]
}

pub fn call_tool(name: &str, _params: Value, _index_dir: &PathBuf) -> Result<String, String> {
    Err(format!("Unknown tool: {name}"))
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
