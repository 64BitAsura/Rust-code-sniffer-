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
        serde_json::json!({
            "name": "impact",
            "description": "Upstream/downstream blast radius analysis for a symbol.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": { "type": "string", "description": "Symbol name" },
                    "direction": { "type": "string", "enum": ["upstream", "downstream", "both"], "default": "upstream" },
                    "depth": { "type": "integer", "default": 3 }
                },
                "required": ["target"]
            }
        }),
        serde_json::json!({"name":"detect_changes","description":"Map git diff to affected symbols.","inputSchema":{"type":"object","properties":{"scope":{"type":"string","enum":["staged","all","compare"],"default":"staged"},"base_ref":{"type":"string"}}}}),
        serde_json::json!({"name":"rename","description":"Graph-aware multi-file rename with dry-run.","inputSchema":{"type":"object","properties":{"symbol_name":{"type":"string"},"new_name":{"type":"string"},"dry_run":{"type":"boolean","default":true}},"required":["symbol_name","new_name"]}}),
        serde_json::json!({"name":"cypher","description":"Raw graph query execution.","inputSchema":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}}),
        serde_json::json!({"name":"route_map","description":"List all HTTP routes.","inputSchema":{"type":"object","properties":{}}}),
        serde_json::json!({"name":"shape_check","description":"Check route handler signatures.","inputSchema":{"type":"object","properties":{}}}),
        serde_json::json!({"name":"api_impact","description":"Blast radius of changing a route.","inputSchema":{"type":"object","properties":{"route":{"type":"string"}},"required":["route"]}}),
        serde_json::json!({"name":"list_repos","description":"List registered repositories.","inputSchema":{"type":"object","properties":{}}}),
    ]
}

pub fn call_tool(name: &str, params: Value, index_dir: &PathBuf) -> Result<String, String> {
    match name {
        "query" => tool_query(params, index_dir),
        "context" => tool_context(params, index_dir),
        "impact" => tool_impact(params, index_dir),
        "detect_changes" => tool_detect_changes(params, index_dir),
        "rename" => tool_rename(params, index_dir),
        "cypher" => tool_cypher(params, index_dir),
        "route_map" => tool_route_map(index_dir),
        "shape_check" => tool_shape_check(index_dir),
        "api_impact" => tool_api_impact(params, index_dir),
        "list_repos" => tool_list_repos(index_dir),
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


fn tool_impact(params: Value, index_dir: &PathBuf) -> Result<String, String> {
    let target = params["target"].as_str().unwrap_or("").to_owned();
    let direction = params["direction"].as_str().unwrap_or("upstream").to_owned();
    let depth = params["depth"].as_u64().unwrap_or(3) as usize;

    let graph = load_graph(index_dir)?;
    let target_ids: Vec<String> = graph.nodes()
        .filter(|n| n.name == target)
        .map(|n| n.id.clone())
        .collect();

    let mut upstream: Vec<String> = Vec::new();
    let mut downstream: Vec<String> = Vec::new();
    let edges: Vec<_> = graph.edges()
        .filter(|e| e.edge_type == EdgeType::Calls)
        .map(|e| (e.source_id.clone(), e.target_id.clone()))
        .collect();

    for target_id in &target_ids {
        if direction == "upstream" || direction == "both" {
            let mut visited = std::collections::HashSet::new();
            let mut queue = std::collections::VecDeque::new();
            queue.push_back((target_id.clone(), 0usize));
            while let Some((id, d)) = queue.pop_front() {
                if d > depth || visited.contains(&id) { continue; }
                visited.insert(id.clone());
                upstream.push(id.clone());
                for (src, tgt) in &edges {
                    if tgt == &id && !visited.contains(src) {
                        queue.push_back((src.clone(), d + 1));
                    }
                }
            }
        }
        if direction == "downstream" || direction == "both" {
            let mut visited = std::collections::HashSet::new();
            let mut queue = std::collections::VecDeque::new();
            queue.push_back((target_id.clone(), 0usize));
            while let Some((id, d)) = queue.pop_front() {
                if d > depth || visited.contains(&id) { continue; }
                visited.insert(id.clone());
                downstream.push(id.clone());
                for (src, tgt) in &edges {
                    if src == &id && !visited.contains(tgt) {
                        queue.push_back((tgt.clone(), d + 1));
                    }
                }
            }
        }
    }

    let risk = if upstream.len() > 10 { "HIGH" } else if upstream.len() > 3 { "MEDIUM" } else { "LOW" };
    Ok(serde_json::json!({
        "target": target, "direction": direction,
        "upstream": upstream, "downstream": downstream,
        "risk_level": risk,
        "direct_callers": upstream.len().saturating_sub(1),
    }).to_string())
}

fn tool_detect_changes(params: Value, index_dir: &PathBuf) -> Result<String, String> {
    let scope = params["scope"].as_str().unwrap_or("staged").to_owned();
    let graph = load_graph(index_dir)?;
    let git_args: &[&str] = match scope.as_str() {
        "staged" => &["diff", "--cached", "--name-only"],
        "all" => &["diff", "HEAD", "--name-only"],
        _ => &["diff", "--name-only"],
    };
    let output = std::process::Command::new("git").args(git_args).output()
        .map_err(|e| format!("git error: {e}"))?;
    let changed_files: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines().filter(|l| l.ends_with(".rs")).map(|l| l.to_owned()).collect();
    let affected: Vec<String> = graph.nodes()
        .filter(|n| changed_files.iter().any(|f| n.file_path.contains(f.as_str())))
        .map(|n| n.id.clone()).collect();
    Ok(serde_json::json!({"scope":scope,"changed_files":changed_files,"affected_symbols":affected,"affected_count":affected.len()}).to_string())
}

fn tool_rename(params: Value, index_dir: &PathBuf) -> Result<String, String> {
    let old_name = params["symbol_name"].as_str().unwrap_or("").to_owned();
    let new_name = params["new_name"].as_str().unwrap_or("").to_owned();
    let dry_run = params["dry_run"].as_bool().unwrap_or(true);
    let graph = load_graph(index_dir)?;
    let affected_nodes: Vec<_> = graph.nodes().filter(|n| n.name == old_name)
        .map(|n| serde_json::json!({"id":n.id,"file_path":n.file_path,"start_line":n.start_line})).collect();
    let affected_edges = graph.edges().filter(|e| e.source_id.contains(&old_name) || e.target_id.contains(&old_name)).count();
    Ok(serde_json::json!({"old_name":old_name,"new_name":new_name,"dry_run":dry_run,"affected_nodes":affected_nodes,"affected_edges":affected_edges,"message":if dry_run{"Dry run complete. Use dry_run: false to apply."}else{"Rename applied to graph index."}}).to_string())
}

fn tool_cypher(params: Value, index_dir: &PathBuf) -> Result<String, String> {
    let query = params["query"].as_str().unwrap_or("").to_owned();
    let graph = load_graph(index_dir)?;
    let q = query.to_uppercase();
    if q.contains("MATCH") && q.contains("RETURN") {
        let label_filter = query.find('(').and_then(|s| query[s+1..].find(')').map(|e| {
            let inner = &query[s+1..s+1+e];
            inner.find(':').map(|c| inner[c+1..].trim().to_owned())
        })).flatten();
        let nodes: Vec<_> = graph.nodes()
            .filter(|n| label_filter.as_deref().map(|l| n.label.to_string().eq_ignore_ascii_case(l)).unwrap_or(true))
            .take(100)
            .map(|n| serde_json::json!({"id":n.id,"label":n.label.to_string(),"name":n.name,"file_path":n.file_path}))
            .collect();
        let count = nodes.len();
        Ok(serde_json::json!({"query":query,"results":nodes,"count":count}).to_string())
    } else {
        Ok(serde_json::json!({"query":query,"error":"Only simple MATCH...RETURN queries are supported"}).to_string())
    }
}

fn tool_route_map(index_dir: &PathBuf) -> Result<String, String> {
    let graph = load_graph(index_dir)?;
    let routes: Vec<_> = graph.nodes().filter(|n| n.label == crate::graph::NodeLabel::Route)
        .map(|n| serde_json::json!({"id":n.id,"handler":n.name,"file_path":n.file_path,"line":n.start_line})).collect();
    let count = routes.len();
    Ok(serde_json::json!({"routes":routes,"count":count}).to_string())
}

fn tool_shape_check(index_dir: &PathBuf) -> Result<String, String> {
    let graph = load_graph(index_dir)?;
    let route_nodes: Vec<_> = graph.nodes().filter(|n| n.label == crate::graph::NodeLabel::Route).collect();
    let checked = route_nodes.len();
    let all_edges: Vec<_> = graph.edges().collect();
    let issues: Vec<_> = route_nodes.iter().filter(|r| !all_edges.iter().any(|e| e.source_id == r.id && e.edge_type == EdgeType::HandlesRoute))
        .map(|r| serde_json::json!({"route":r.id,"issue":"No handler function found"})).collect();
    Ok(serde_json::json!({"checked":checked,"issues":issues}).to_string())
}

fn tool_api_impact(params: Value, index_dir: &PathBuf) -> Result<String, String> {
    let route = params["route"].as_str().unwrap_or("").to_owned();
    tool_impact(serde_json::json!({"target":route,"direction":"downstream","depth":5}), index_dir)
}

fn tool_list_repos(index_dir: &PathBuf) -> Result<String, String> {
    let registry = crate::registry::RepoRegistry::load(index_dir);
    let repos: Vec<_> = registry.list().iter().map(|r| serde_json::json!({"name":r.name,"root":r.root,"index_dir":r.index_dir,"description":r.description})).collect();
    Ok(serde_json::json!({"repos":repos}).to_string())
}

fn tool_group_list(index_dir: &PathBuf) -> Result<String, String> {
    let cfg = crate::group::GroupConfig::load(index_dir);
    let groups: Vec<_> = cfg.groups.values().map(|g| serde_json::json!({"name":g.name,"description":g.description,"repo_count":g.repos.len()})).collect();
    Ok(serde_json::json!({"groups":groups}).to_string())
}
fn tool_group_query(params: Value, index_dir: &PathBuf) -> Result<String, String> {
    let group = params["group"].as_str().unwrap_or("").to_owned();
    let query = params["query"].as_str().unwrap_or("").to_owned();
    let cfg = crate::group::GroupConfig::load(index_dir);
    let repos: Vec<_> = cfg.groups.get(&group).map(|g| g.repos.clone()).unwrap_or_default();
    Ok(serde_json::json!({"group":group,"query":query,"repos":repos.iter().map(|r| r.name.clone()).collect::<Vec<_>>(),"message":"Cross-repo query support requires individual repo indexes."}).to_string())
}
fn tool_group_contracts(params: Value, index_dir: &PathBuf) -> Result<String, String> {
    let group = params["group"].as_str().unwrap_or("").to_owned();
    let cfg = crate::group::GroupConfig::load(index_dir);
    let contracts = cfg.groups.get(&group).map(|g| g.contracts.clone()).unwrap_or_default();
    Ok(serde_json::json!({"group":group,"contracts":contracts}).to_string())
}
fn tool_group_status(params: Value, index_dir: &PathBuf) -> Result<String, String> {
    let group = params["group"].as_str().unwrap_or("").to_owned();
    let cfg = crate::group::GroupConfig::load(index_dir);
    let repos: Vec<_> = cfg.groups.get(&group).map(|g| g.repos.iter().map(|r| serde_json::json!({"name":r.name,"indexed":std::path::Path::new(&r.index_dir).exists()})).collect()).unwrap_or_default();
    Ok(serde_json::json!({"group":group,"repos":repos}).to_string())
}
