//! Directory-walking indexer that wires the parser and incremental state together.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::error::SnifferError;
use crate::graph::store::AdjacencyStore;
use crate::graph::{Edge, EdgeType, GraphStore, Node, NodeLabel};
use crate::incremental::{
    diff_files, fingerprint, load_cached_symbols, save_symbols, HashState,
};
use crate::meta::IndexMeta;
use crate::parser::parse_file;
use crate::symbols::{FileSymbols, SymbolKind};

/// Options that control how the indexer behaves.
#[derive(Debug, Clone)]
pub struct IndexOptions {
    /// Root directory to scan for `*.rs` files.
    pub root: PathBuf,
    /// Directory where hash state and cached symbols are stored.
    pub index_dir: PathBuf,
    /// When `true`, only re-parse files whose content has changed.
    pub incremental: bool,
    /// When `true`, emit progress messages to stderr.
    pub verbose: bool,
}

/// Summary of an indexing run.
#[derive(Debug, Default)]
pub struct IndexSummary {
    /// Total number of `.rs` files discovered.
    pub total_files: usize,
    /// Files that were (re-)parsed in this run.
    pub parsed_files: usize,
    /// Files that were skipped because their content was unchanged.
    pub skipped_files: usize,
    /// Total symbols extracted across all files.
    pub total_symbols: usize,
    /// Files that were removed from the index because they no longer exist.
    pub removed_files: usize,
    /// Total nodes in the graph after this run.
    pub graph_nodes: usize,
    /// Total edges in the graph after this run.
    pub graph_edges: usize,
}

/// Run a full or incremental index of a Rust project.
///
/// Returns the complete symbol list for every indexed file plus a summary.
pub fn run_index(opts: &IndexOptions) -> Result<(Vec<FileSymbols>, IndexSummary), SnifferError> {
    // ── 1. Discover all *.rs files ────────────────────────────────────────────
    let file_list = collect_rust_files(&opts.root)?;

    // ── 2. Read file contents and compute fingerprints ────────────────────────
    let mut file_data: Vec<(String, Vec<u8>)> = Vec::with_capacity(file_list.len());
    for path in &file_list {
        let canonical = canonical_key(path, &opts.root);
        let content = fs::read(path)
            .map_err(|e| SnifferError::Io(format!("reading {}: {e}", path.display())))?;
        file_data.push((canonical, content));
    }

    let total_files = file_data.len();

    // ── 3. Load incremental state (if enabled) ────────────────────────────────
    let state = if opts.incremental {
        HashState::load(&opts.index_dir)?
    } else {
        HashState::default()
    };

    // ── 4. Diff: classify files as changed vs unchanged ───────────────────────
    let diff = diff_files(&file_data, &state);

    // ── 5. Load previous symbols for unchanged files (incremental fast-path) ──
    let cached: HashMap<String, FileSymbols> = if opts.incremental {
        load_cached_symbols(&opts.index_dir)
            .unwrap_or_default()
            .into_iter()
            .map(|fs| (fs.path.clone(), fs))
            .collect()
    } else {
        HashMap::new()
    };

    // ── 6. Parse changed files ────────────────────────────────────────────────
    let mut results: Vec<FileSymbols> = Vec::with_capacity(total_files);
    let mut parsed_count = 0usize;
    let mut skipped_count = 0usize;

    // Build a map from canonical path → raw content for changed files.
    let changed_set: std::collections::HashSet<String> = diff
        .changed
        .iter()
        .map(|p| canonical_key(p, &opts.root))
        .collect();

    for (canonical, content) in &file_data {
        if changed_set.contains(canonical) {
            // This file is new or modified — parse it.
            let hash = diff.hashes[canonical].clone();
            let source = String::from_utf8_lossy(content);
            if opts.verbose {
                eprintln!("  parsing  {canonical}");
            }
            match parse_file(canonical, &source, hash) {
                Ok(fs) => results.push(fs),
                Err(e) => {
                    eprintln!("  warning: skipping {canonical} — {e}");
                }
            }
            parsed_count += 1;
        } else if let Some(cached_fs) = cached.get(canonical) {
            // Unchanged — reuse cached symbols.
            if opts.verbose {
                eprintln!("  cached   {canonical}");
            }
            results.push(cached_fs.clone());
            skipped_count += 1;
        } else {
            // No cache available even though hash matched (shouldn't happen in normal
            // operation, but handle gracefully by re-parsing).
            let hash = diff.hashes[canonical].clone();
            let source = String::from_utf8_lossy(content);
            if opts.verbose {
                eprintln!("  reparse  {canonical} (no cache)");
            }
            if let Ok(fs) = parse_file(canonical, &source, hash) {
                results.push(fs);
            }
            parsed_count += 1;
        }
    }

    // ── 7. Purge stale entries from state ─────────────────────────────────────
    let current_keys: Vec<String> = file_data.iter().map(|(k, _)| k.clone()).collect();
    let stale: Vec<String> = state
        .stale_files(&current_keys)
        .iter()
        .map(|s| s.to_string())
        .collect();
    let removed_count = stale.len();

    // ── 8. Persist updated hash state and symbol cache ────────────────────────
    if opts.incremental {
        let mut new_state = HashState::default();
        for (path, hash) in &diff.hashes {
            new_state.update(path, hash);
        }
        new_state.save(&opts.index_dir)?;
        save_symbols(&opts.index_dir, &results)?;
    }

    let total_symbols = results.iter().map(|fs| fs.symbols.len()).sum();

    // ── 9. Build / update the graph store ─────────────────────────────────────
    // Load whatever was persisted from a previous run so that unchanged-file
    // symbols are already present in the graph.
    let mut graph = AdjacencyStore::load(&opts.index_dir)?;

    // Purge stale file nodes from the graph so deleted files don't linger.
    for stale_path in &stale {
        graph.remove_by_file(stale_path);
    }

    // Populate graph from the full result set (upsert is idempotent).
    for file_syms in &results {
        populate_graph(&mut graph, file_syms);
    }

    // ── 9b. Resolve CALLS edges ────────────────────────────────────────────────
    // Build a global name → Vec<node_id> lookup table from all known symbols.
    let name_index = build_name_index(&results);

    // Emit CALLS edges for every call site in every (re-)parsed file.
    for file_syms in &results {
        resolve_calls(&mut graph, file_syms, &name_index);
    }

    // ── 9c. Resolve IMPORTS edges ─────────────────────────────────────────────
    // Build a set of all known file paths so we can map Rust module paths to
    // canonical file node IDs.
    let all_file_paths: HashSet<String> = results.iter().map(|fs| fs.path.clone()).collect();

    // Emit IMPORTS edges for every `use` declaration in every file.
    for file_syms in &results {
        resolve_imports(&mut graph, file_syms, &all_file_paths);
    }

    // Persist the updated graph.
    graph.save(&opts.index_dir)?;

    let graph_nodes = graph.node_count();
    let graph_edges = graph.edge_count();

    // ── 10. Always write meta.json so status / serve have fresh stats ──────────
    let root_str = opts.root.to_string_lossy().into_owned();
    let meta = IndexMeta::new(root_str, total_files, total_symbols, graph_nodes, graph_edges);
    // Best-effort: a metadata write failure is not fatal.
    let _ = meta.save(&opts.index_dir);

    let summary = IndexSummary {
        total_files,
        parsed_files: parsed_count,
        skipped_files: skipped_count,
        total_symbols,
        removed_files: removed_count,
        graph_nodes,
        graph_edges,
    };

    Ok((results, summary))
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Walk `root` and return all `*.rs` paths (sorted for determinism).
fn collect_rust_files(root: &Path) -> Result<Vec<PathBuf>, SnifferError> {
    let mut paths = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path().to_path_buf();
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

/// Produce a canonical path key relative to `root`.
///
/// Falls back to the full path string if stripping the root prefix fails.
fn canonical_key(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

/// Compute a fingerprint for a file on disk.
pub fn fingerprint_file(path: &Path) -> Result<String, SnifferError> {
    let content = fs::read(path)
        .map_err(|e| SnifferError::Io(format!("reading {}: {e}", path.display())))?;
    Ok(fingerprint(&content))
}

/// Populate `graph` with the nodes and edges derived from a single file's
/// extracted symbols.
///
/// Creates:
/// * One `File` node for the source file itself.
/// * One node per symbol (function, struct, enum, …).
/// * `DEFINES` edges from the `File` node to each top-level symbol.
/// * `HAS_METHOD` edges from `Impl`/`TraitImpl` nodes to their enclosed
///   `Function` symbols (not yet tracked by the parser, so this is a
///   best-effort pass).
/// * `HAS_PROPERTY` edges from `Struct`/`Enum` nodes to `Field` symbols.
/// * `IMPLEMENTS` edges from `TraitImpl` nodes to a synthetic trait node.
fn populate_graph(graph: &mut AdjacencyStore, file_syms: &FileSymbols) {
    let file_path = &file_syms.path;

    // ── File node ──────────────────────────────────────────────────────────────
    let file_node_id = format!("file:{file_path}");
    graph.upsert_node(Node {
        id: file_node_id.clone(),
        label: NodeLabel::File,
        name: file_path.clone(),
        file_path: String::new(),
        start_line: 0,
        end_line: 0,
    });

    // ── Symbol nodes + DEFINES edges ──────────────────────────────────────────
    // We also collect impl/struct/enum containers to attach child edges later.
    // Since the parser doesn't emit hierarchical parent info, we use a
    // line-range heuristic: a symbol is a "child" of the closest preceding
    // container whose end_line >= symbol.start_line.

    // Build a vector of (node_id, symbol) for further edge analysis.
    let mut sym_ids: Vec<(String, &crate::symbols::Symbol)> = Vec::new();

    for sym in &file_syms.symbols {
        let label = symbol_kind_to_label(&sym.kind);
        let sym_id = format!("{label}:{file_path}::{}", sym.name);

        graph.upsert_node(Node {
            id: sym_id.clone(),
            label,
            name: sym.name.clone(),
            file_path: file_path.clone(),
            start_line: sym.start_line,
            end_line: sym.end_line,
        });

        // File DEFINES every top-level symbol.
        let edge_id = format!("{file_node_id}--DEFINES-->{sym_id}");
        graph.upsert_edge(Edge {
            id: edge_id,
            source_id: file_node_id.clone(),
            target_id: sym_id.clone(),
            edge_type: EdgeType::Defines,
            confidence: 1.0,
            reason: String::new(),
        });

        sym_ids.push((sym_id, sym));
    }

    // ── Containment / membership edges ────────────────────────────────────────
    // For each symbol, find the enclosing container (the last container in the
    // list whose line range fully encompasses this symbol).
    for (child_id, child_sym) in &sym_ids {
        // Find the nearest enclosing impl / struct / enum / trait / module.
        let container = sym_ids.iter().find(|(_, s)| {
            is_container(&s.kind)
                && s.start_line <= child_sym.start_line
                && s.end_line >= child_sym.end_line
                && s.name != child_sym.name
        });

        if let Some((container_id, container_sym)) = container {
            let etype = match (&container_sym.kind, &child_sym.kind) {
                (SymbolKind::Struct | SymbolKind::Enum, SymbolKind::Field) => {
                    EdgeType::HasProperty
                }
                (SymbolKind::Impl | SymbolKind::TraitImpl, SymbolKind::Function) => {
                    EdgeType::HasMethod
                }
                _ => EdgeType::Contains,
            };
            let edge_id = format!("{container_id}--{}-->{child_id}", etype);
            graph.upsert_edge(Edge {
                id: edge_id,
                source_id: container_id.clone(),
                target_id: child_id.clone(),
                edge_type: etype,
                confidence: 1.0,
                reason: String::new(),
            });
        }
    }

    // ── IMPLEMENTS edges for TraitImpl nodes ──────────────────────────────────
    for (impl_id, sym) in &sym_ids {
        if sym.kind == SymbolKind::TraitImpl {
            if let Some(trait_name) = &sym.trait_name {
                // Emit a synthetic trait node if it isn't already present.
                let trait_node_id = format!("Trait::{trait_name}");
                graph.upsert_node(Node {
                    id: trait_node_id.clone(),
                    label: NodeLabel::Trait,
                    name: trait_name.clone(),
                    file_path: String::new(),
                    start_line: 0,
                    end_line: 0,
                });
                let edge_id = format!("{impl_id}--IMPLEMENTS-->{trait_node_id}");
                graph.upsert_edge(Edge {
                    id: edge_id,
                    source_id: impl_id.clone(),
                    target_id: trait_node_id,
                    edge_type: EdgeType::Implements,
                    confidence: 1.0,
                    reason: String::new(),
                });
            }
        }
    }
}

/// Convert a [`SymbolKind`] to the corresponding [`NodeLabel`].
fn symbol_kind_to_label(kind: &SymbolKind) -> NodeLabel {
    match kind {
        SymbolKind::Function => NodeLabel::Function,
        SymbolKind::Struct => NodeLabel::Struct,
        SymbolKind::Enum => NodeLabel::Enum,
        SymbolKind::Trait => NodeLabel::Trait,
        SymbolKind::Impl | SymbolKind::TraitImpl => NodeLabel::Impl,
        SymbolKind::Module => NodeLabel::Module,
        SymbolKind::TypeAlias => NodeLabel::TypeAlias,
        SymbolKind::Constant => NodeLabel::Constant,
        SymbolKind::Static => NodeLabel::Static,
        SymbolKind::Macro => NodeLabel::Macro,
        SymbolKind::Field => NodeLabel::Field,
    }
}

/// Return `true` if a symbol kind can act as a container for child symbols.
fn is_container(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Impl
            | SymbolKind::TraitImpl
            | SymbolKind::Module
            | SymbolKind::Trait
    )
}

// ─── CALLS edge helpers ────────────────────────────────────────────────────────

/// Build a map from symbol name → list of node UIDs across all indexed files.
///
/// Only callable symbols (functions, and also structs/enums to catch
/// constructor-style calls) are indexed.
fn build_name_index(all_files: &[FileSymbols]) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();

    for file_syms in all_files {
        let file_path = &file_syms.path;
        for sym in &file_syms.symbols {
            // Only index symbols that can meaningfully be callee targets.
            if !matches!(
                sym.kind,
                SymbolKind::Function
                    | SymbolKind::Struct
                    | SymbolKind::Enum
                    | SymbolKind::Macro
            ) {
                continue;
            }
            let label = symbol_kind_to_label(&sym.kind);
            let node_id = format!("{label}:{file_path}::{}", sym.name);
            index.entry(sym.name.clone()).or_default().push(node_id);
        }
    }

    index
}

/// Emit `CALLS` edges from the call sites in `file_syms` into `graph`.
///
/// * Exact match (one candidate)  → `confidence = 1.0`
/// * Ambiguous match (>1 candidate) → `confidence = 0.7` for every candidate
/// * No match                     → edge is skipped
fn resolve_calls(
    graph: &mut AdjacencyStore,
    file_syms: &FileSymbols,
    name_index: &HashMap<String, Vec<String>>,
) {
    let file_path = &file_syms.path;

    for call in &file_syms.calls {
        // Skip call sites that are not inside a named function.
        if call.caller_name.is_empty() {
            continue;
        }

        let caller_id = format!("Function:{file_path}::{}", call.caller_name);

        let candidates = match name_index.get(&call.callee_name) {
            Some(c) if !c.is_empty() => c,
            _ => continue,
        };

        let confidence = if candidates.len() == 1 { 1.0 } else { 0.7 };

        for callee_id in candidates {
            // Skip self-recursive edges that trivially appear as "calls itself".
            if callee_id == &caller_id {
                continue;
            }

            let edge_id = format!("{caller_id}--CALLS-->{callee_id}:L{}", call.line);
            graph.upsert_edge(Edge {
                id: edge_id,
                source_id: caller_id.clone(),
                target_id: callee_id.clone(),
                edge_type: EdgeType::Calls,
                confidence,
                reason: "rust-call".to_owned(),
            });
        }
    }
}

// ─── IMPORTS edge helpers ──────────────────────────────────────────────────────

/// Emit `IMPORTS` edges from the `use` declarations in `file_syms` into `graph`.
///
/// * Normal import  → `confidence = 1.0`
/// * Glob import (`::*`) → `confidence = 0.5`
/// * Unresolvable path   → edge is silently skipped
fn resolve_imports(
    graph: &mut AdjacencyStore,
    file_syms: &FileSymbols,
    all_files: &HashSet<String>,
) {
    let file_path = &file_syms.path;
    let file_node_id = format!("file:{file_path}");

    for import in &file_syms.imports {
        let target_path = match resolve_import_path(file_path, &import.raw_path, all_files) {
            Some(p) => p,
            None => continue,
        };

        // Skip self-imports (e.g. `use self::*` pointing to the same file).
        if target_path == *file_path {
            continue;
        }

        let target_node_id = format!("file:{target_path}");
        let confidence = if import.is_glob { 0.5 } else { 1.0 };
        let edge_id = format!("{file_node_id}--IMPORTS-->{target_node_id}");
        graph.upsert_edge(Edge {
            id: edge_id,
            source_id: file_node_id.clone(),
            target_id: target_node_id,
            edge_type: EdgeType::Imports,
            confidence,
            reason: "rust-use".to_owned(),
        });
    }
}

/// Resolve a single import path string to a canonical file path.
///
/// Handles `crate::`, `super::`, `self::` prefixes and bare `::` paths.
/// Returns `None` when no matching file is found in `all_files`.
fn resolve_import_path(
    current_file: &str,
    raw_path: &str,
    all_files: &HashSet<String>,
) -> Option<String> {
    // Strip an alias suffix (`Foo as Bar` → `Foo`).
    let path = raw_path
        .split(" as ")
        .next()
        .unwrap_or(raw_path)
        .trim();

    if path.starts_with("crate::") {
        // crate:: → resolve from src/ (standard Rust layout).
        let tail = path[7..].replace("::", "/");
        let from_src = try_module_path(&format!("src/{tail}"), all_files);
        if from_src.is_some() {
            return from_src;
        }
        return try_module_path(&tail, all_files);
    }

    if path.starts_with("super::") {
        // super:: → parent directory of the current file's module.
        let parts: Vec<&str> = current_file.split('/').collect();
        // Drop the filename and one more directory level.
        let parent = if parts.len() >= 2 { &parts[..parts.len() - 2] } else { &[] };
        let tail = path[7..].replace("::", "/");
        let full = if parent.is_empty() {
            tail
        } else {
            format!("{}/{}", parent.join("/"), tail)
        };
        return try_module_path(&full, all_files);
    }

    if path.starts_with("self::") {
        // self:: → same directory as the current file.
        let parts: Vec<&str> = current_file.split('/').collect();
        let dir = if parts.len() >= 2 { &parts[..parts.len() - 1] } else { &[] };
        let tail = path[6..].replace("::", "/");
        let full = if dir.is_empty() {
            tail
        } else {
            format!("{}/{}", dir.join("/"), tail)
        };
        return try_module_path(&full, all_files);
    }

    if path.contains("::") {
        // Generic qualified path — convert `::` to `/` and try direct then
        // suffix matching.
        let rust_path = path.replace("::", "/");

        // Direct match from the repo root.
        if let Some(found) = try_module_path(&rust_path, all_files) {
            return Some(found);
        }

        // Suffix match: any known file that ends with this path.
        for f in all_files {
            let base = f.trim_end_matches(".rs").trim_end_matches("/mod");
            if base.ends_with(&rust_path) || f.ends_with(&format!("{rust_path}.rs")) {
                return Some(f.clone());
            }
        }
    }

    None
}

/// Try to resolve a Rust module path to an existing file in `all_files`.
///
/// Attempts in order:
/// 1. `{path}.rs`
/// 2. `{path}/mod.rs`
/// 3. Recursively with the last path segment stripped (the last segment may be
///    a symbol name rather than a module name, e.g. `models::User` → `models`).
fn try_module_path(path: &str, all_files: &HashSet<String>) -> Option<String> {
    let as_rs = format!("{path}.rs");
    if all_files.contains(&as_rs) {
        return Some(as_rs);
    }

    let as_mod = format!("{path}/mod.rs");
    if all_files.contains(&as_mod) {
        return Some(as_mod);
    }

    // Strip the last segment and retry (in case it was a symbol name).
    if let Some(sep) = path.rfind('/') {
        let parent = &path[..sep];
        if !parent.is_empty() {
            return try_module_path(parent, all_files);
        }
    }

    None
}
