//! Wiki generation — produce a structured markdown wiki from the knowledge graph.
//!
//! Generates one page per community, one page per process, and a top-level
//! `index.md` that links everything together.

use std::fs;
use std::path::Path;

use crate::community::Community;
use crate::error::SnifferError;
use crate::graph::store::AdjacencyStore;
use crate::graph::{EdgeType, GraphStore};
use crate::process::Process;

const COMMUNITIES_FILE: &str = "communities.json";
const PROCESSES_FILE: &str = "processes.json";

/// Generate a markdown wiki from the knowledge graph.
///
/// Reads communities and processes from `index_dir` and writes markdown files
/// to `out_dir`.  Pass `include_llm_hints = false` to skip AI-summary
/// placeholder sections.
pub fn generate_wiki(
    index_dir: &Path,
    out_dir: &Path,
    include_llm_hints: bool,
) -> Result<(), SnifferError> {
    fs::create_dir_all(out_dir)
        .map_err(|e| SnifferError::Io(format!("creating wiki dir: {e}")))?;

    // Load communities
    let communities = load_communities(index_dir);
    // Load processes
    let processes = load_processes(index_dir);
    // Load graph for symbol details
    let graph = AdjacencyStore::load(index_dir)?;

    // Generate community pages
    let mut community_links: Vec<(String, String)> = Vec::new(); // (label, filename)
    for community in &communities {
        let filename = format!(
            "community-{}.md",
            slugify(&community.heuristic_label)
        );
        let content = render_community_page(community, &graph, include_llm_hints);
        fs::write(out_dir.join(&filename), content)
            .map_err(|e| SnifferError::Io(format!("writing {filename}: {e}")))?;
        community_links.push((community.heuristic_label.clone(), filename));
    }

    // Generate process pages
    let mut process_links: Vec<(String, String)> = Vec::new(); // (name, filename)
    for process in &processes {
        let filename = format!("process-{}.md", slugify(&process.name));
        let content = render_process_page(process, &graph, include_llm_hints);
        fs::write(out_dir.join(&filename), content)
            .map_err(|e| SnifferError::Io(format!("writing {filename}: {e}")))?;
        process_links.push((process.name.clone(), filename));
    }

    // Generate index page
    let index_content = render_index_page(&community_links, &process_links);
    fs::write(out_dir.join("index.md"), index_content)
        .map_err(|e| SnifferError::Io(format!("writing index.md: {e}")))?;

    Ok(())
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn render_index_page(
    community_links: &[(String, String)],
    process_links: &[(String, String)],
) -> String {
    let mut out = String::new();
    out.push_str("# ast-line Knowledge Graph Wiki\n\n");
    out.push_str("Auto-generated from the indexed knowledge graph.\n\n");

    out.push_str("## Communities\n\n");
    if community_links.is_empty() {
        out.push_str("_No communities detected. Run `ast-line index` first._\n\n");
    } else {
        for (label, file) in community_links {
            out.push_str(&format!("- [{label}](./{file})\n"));
        }
        out.push('\n');
    }

    out.push_str("## Execution Flows\n\n");
    if process_links.is_empty() {
        out.push_str("_No execution flows detected. Run `ast-line index` first._\n\n");
    } else {
        for (name, file) in process_links {
            out.push_str(&format!("- [{name}](./{file})\n"));
        }
        out.push('\n');
    }

    out
}

fn render_community_page(
    community: &Community,
    graph: &AdjacencyStore,
    include_llm_hints: bool,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Community: {}\n\n", community.heuristic_label));
    out.push_str(&format!(
        "**Cohesion:** {:.2}  |  **Symbol count:** {}\n\n",
        community.cohesion,
        community.symbol_ids.len()
    ));

    if !community.keywords.is_empty() {
        out.push_str(&format!(
            "**Keywords:** {}\n\n",
            community.keywords.join(", ")
        ));
    }

    if include_llm_hints {
        out.push_str("> _AI-generated description: run `ast-line augment` to enrich this page._\n\n");
    }

    out.push_str("## Symbols\n\n");
    out.push_str("| Symbol | Label | File | Lines |\n");
    out.push_str("|--------|-------|------|-------|\n");
    for sym_id in &community.symbol_ids {
        if let Some(node) = graph.nodes().find(|n| &n.id == sym_id) {
            out.push_str(&format!(
                "| `{}` | {} | `{}` | {}–{} |\n",
                node.name,
                node.label,
                node.file_path,
                node.start_line,
                node.end_line
            ));
        }
    }
    out.push('\n');

    // Entry points within this community
    let entry_points: Vec<_> = community
        .symbol_ids
        .iter()
        .filter_map(|id| graph.nodes().find(|n| &n.id == id))
        .filter(|n| n.entry_point_score > 0.0)
        .collect();
    if !entry_points.is_empty() {
        out.push_str("## Entry Points\n\n");
        for n in &entry_points {
            out.push_str(&format!(
                "- `{}` (score: {:.2})\n",
                n.name, n.entry_point_score
            ));
        }
        out.push('\n');
    }

    out.push_str("[← Back to index](./index.md)\n");
    out
}

fn render_process_page(
    process: &Process,
    graph: &AdjacencyStore,
    include_llm_hints: bool,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Process: {}\n\n", process.name));
    out.push_str(&format!(
        "**Entry point:** `{}`  |  **Steps:** {}\n\n",
        process.entry_point_id, process.step_count
    ));

    if include_llm_hints {
        out.push_str("> _AI-generated description: run `ast-line augment` to enrich this page._\n\n");
    }

    out.push_str("## Call Chain\n\n");
    out.push_str("| Step | Symbol | File | Lines |\n");
    out.push_str("|------|--------|------|-------|\n");
    for (i, step_id) in process.steps.iter().enumerate() {
        if let Some(node) = graph.nodes().find(|n| &n.id == step_id) {
            out.push_str(&format!(
                "| {} | `{}` | `{}` | {}–{} |\n",
                i + 1,
                node.name,
                node.file_path,
                node.start_line,
                node.end_line
            ));
        } else {
            out.push_str(&format!("| {} | `{step_id}` | — | — |\n", i + 1));
        }
    }
    out.push('\n');

    // Callee breakdown from CALLS edges among process steps
    let step_set: std::collections::HashSet<&String> = process.steps.iter().collect();
    let internal_calls: Vec<_> = graph
        .edges()
        .filter(|e| {
            e.edge_type == EdgeType::Calls
                && step_set.contains(&e.source_id)
                && step_set.contains(&e.target_id)
        })
        .collect();

    if !internal_calls.is_empty() {
        out.push_str("## Internal Calls\n\n");
        out.push_str("| Caller | Callee |\n");
        out.push_str("|--------|--------|\n");
        for edge in &internal_calls {
            let src_name = graph
                .nodes()
                .find(|n| n.id == edge.source_id)
                .map(|n| n.name.as_str())
                .unwrap_or(&edge.source_id);
            let tgt_name = graph
                .nodes()
                .find(|n| n.id == edge.target_id)
                .map(|n| n.name.as_str())
                .unwrap_or(&edge.target_id);
            out.push_str(&format!("| `{src_name}` | `{tgt_name}` |\n"));
        }
        out.push('\n');
    }

    out.push_str("[← Back to index](./index.md)\n");
    out
}

fn load_communities(index_dir: &Path) -> Vec<Community> {
    let path = index_dir.join(COMMUNITIES_FILE);
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn load_processes(index_dir: &Path) -> Vec<Process> {
    let path = index_dir.join(PROCESSES_FILE);
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
