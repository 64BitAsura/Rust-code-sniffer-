//! Integration tests for the ast-line indexer.

use std::fs;

use tempfile::TempDir;

use ast_line::indexer::{run_index, IndexOptions};
use ast_line::incremental::{fingerprint, HashState};
use ast_line::parser::parse_file;
use ast_line::symbols::SymbolKind;

// ─── Fixture helpers ──────────────────────────────────────────────────────────

fn make_project(dir: &TempDir, files: &[(&str, &str)]) {
    for (name, content) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }
}

fn index_dir(tmp: &TempDir) -> std::path::PathBuf {
    tmp.path().join(".ast-line")
}

// ─── Parser unit tests ────────────────────────────────────────────────────────

#[test]
fn parses_top_level_function() {
    let src = r#"
pub fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}
"#;
    let hash = fingerprint(src.as_bytes());
    let result = parse_file("test.rs", src, hash).unwrap();

    let fns: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Function && s.name == "greet")
        .collect();
    assert_eq!(fns.len(), 1, "expected exactly one 'greet' function");
    assert_eq!(fns[0].return_type.as_deref(), Some("String"));
}

#[test]
fn parses_struct_with_fields() {
    let src = r#"
pub struct User {
    pub name: String,
    age: u32,
}
"#;
    let hash = fingerprint(src.as_bytes());
    let result = parse_file("test.rs", src, hash).unwrap();

    let structs: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Struct)
        .collect();
    assert_eq!(structs.len(), 1);
    assert_eq!(structs[0].name, "User");

    let fields: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Field)
        .collect();
    assert_eq!(fields.len(), 2, "expected 2 fields (name, age)");
}

#[test]
fn parses_enum() {
    let src = r#"
pub enum Color { Red, Green, Blue }
"#;
    let hash = fingerprint(src.as_bytes());
    let result = parse_file("test.rs", src, hash).unwrap();

    let enums: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Enum)
        .collect();
    assert_eq!(enums.len(), 1);
    assert_eq!(enums[0].name, "Color");
}

#[test]
fn parses_trait() {
    let src = r#"
pub trait Greetable {
    fn greet(&self) -> String;
}
"#;
    let hash = fingerprint(src.as_bytes());
    let result = parse_file("test.rs", src, hash).unwrap();

    let traits: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Trait)
        .collect();
    assert_eq!(traits.len(), 1);
    assert_eq!(traits[0].name, "Greetable");
}

#[test]
fn parses_impl_and_trait_impl() {
    let src = r#"
struct Dog;

impl Dog {
    fn bark(&self) {}
}

impl Animal for Dog {
    fn speak(&self) {}
}
"#;
    let hash = fingerprint(src.as_bytes());
    let result = parse_file("test.rs", src, hash).unwrap();

    let impls: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Impl)
        .collect();
    assert_eq!(impls.len(), 1, "expected one plain impl");

    let trait_impls: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::TraitImpl)
        .collect();
    assert_eq!(trait_impls.len(), 1);
    assert_eq!(trait_impls[0].trait_name.as_deref(), Some("Animal"));
}

#[test]
fn detects_async_function() {
    let src = r#"
async fn fetch_data() -> Vec<u8> { vec![] }
"#;
    let hash = fingerprint(src.as_bytes());
    let result = parse_file("test.rs", src, hash).unwrap();

    let fns: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Function && s.name == "fetch_data")
        .collect();
    assert_eq!(fns.len(), 1);
    assert!(fns[0].is_async, "expected is_async to be true");
}

#[test]
fn parses_const_and_static() {
    let src = r#"
pub const MAX_SIZE: usize = 1024;
static COUNTER: u32 = 0;
"#;
    let hash = fingerprint(src.as_bytes());
    let result = parse_file("test.rs", src, hash).unwrap();

    let consts: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Constant)
        .collect();
    assert_eq!(consts.len(), 1);
    assert_eq!(consts[0].name, "MAX_SIZE");

    let statics: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Static)
        .collect();
    assert_eq!(statics.len(), 1);
    assert_eq!(statics[0].name, "COUNTER");
}

// ─── Indexer integration tests ────────────────────────────────────────────────

#[test]
fn full_index_discovers_all_files() {
    let tmp = TempDir::new().unwrap();
    make_project(
        &tmp,
        &[
            ("src/lib.rs", "pub fn lib_fn() {}"),
            ("src/main.rs", "fn main() {}"),
        ],
    );

    let opts = IndexOptions {
        root: tmp.path().to_path_buf(),
        index_dir: index_dir(&tmp),
        incremental: false,
        verbose: false,
    };

    let (symbols, summary) = run_index(&opts).unwrap();
    assert_eq!(summary.total_files, 2);
    assert_eq!(summary.parsed_files, 2);
    assert_eq!(summary.skipped_files, 0);
    assert!(summary.total_symbols >= 2, "at least lib_fn and main");

    let names: Vec<&str> = symbols
        .iter()
        .flat_map(|fs| fs.symbols.iter())
        .filter(|s| s.kind == SymbolKind::Function)
        .map(|s| s.name.as_str())
        .collect();
    assert!(names.contains(&"lib_fn"), "expected lib_fn");
    assert!(names.contains(&"main"), "expected main");
}

#[test]
fn incremental_index_skips_unchanged_files() {
    let tmp = TempDir::new().unwrap();
    make_project(
        &tmp,
        &[
            ("src/a.rs", "pub fn a() {}"),
            ("src/b.rs", "pub fn b() {}"),
        ],
    );

    let opts = IndexOptions {
        root: tmp.path().to_path_buf(),
        index_dir: index_dir(&tmp),
        incremental: true,
        verbose: false,
    };

    // First run — everything is parsed.
    let (_, summary1) = run_index(&opts).unwrap();
    assert_eq!(summary1.parsed_files, 2);
    assert_eq!(summary1.skipped_files, 0);

    // Second run without changes — everything should be skipped.
    let (_, summary2) = run_index(&opts).unwrap();
    assert_eq!(summary2.skipped_files, 2, "both files should be cached");
    assert_eq!(summary2.parsed_files, 0);
}

#[test]
fn incremental_index_reparses_changed_file() {
    let tmp = TempDir::new().unwrap();
    make_project(
        &tmp,
        &[
            ("src/a.rs", "pub fn a_v1() {}"),
            ("src/b.rs", "pub fn b() {}"),
        ],
    );

    let opts = IndexOptions {
        root: tmp.path().to_path_buf(),
        index_dir: index_dir(&tmp),
        incremental: true,
        verbose: false,
    };

    // Initial index.
    run_index(&opts).unwrap();

    // Modify one file.
    fs::write(tmp.path().join("src/a.rs"), "pub fn a_v2() {}").unwrap();

    let (symbols, summary) = run_index(&opts).unwrap();
    assert_eq!(summary.parsed_files, 1, "only a.rs should be re-parsed");
    assert_eq!(summary.skipped_files, 1, "b.rs should be cached");

    let fn_names: Vec<&str> = symbols
        .iter()
        .flat_map(|fs| fs.symbols.iter())
        .filter(|s| s.kind == SymbolKind::Function)
        .map(|s| s.name.as_str())
        .collect();
    assert!(fn_names.contains(&"a_v2"), "updated function should appear");
    assert!(!fn_names.contains(&"a_v1"), "old function should be gone");
    assert!(fn_names.contains(&"b"), "unchanged function should still appear");
}

#[test]
fn incremental_index_tracks_new_files() {
    let tmp = TempDir::new().unwrap();
    make_project(&tmp, &[("src/existing.rs", "pub fn existing() {}")]);

    let opts = IndexOptions {
        root: tmp.path().to_path_buf(),
        index_dir: index_dir(&tmp),
        incremental: true,
        verbose: false,
    };

    run_index(&opts).unwrap();

    // Add a new file.
    fs::write(tmp.path().join("src/new_file.rs"), "pub fn brand_new() {}").unwrap();

    let (symbols, summary) = run_index(&opts).unwrap();
    assert_eq!(summary.total_files, 2);
    assert_eq!(summary.parsed_files, 1, "only new_file.rs should be parsed");
    assert_eq!(summary.skipped_files, 1);

    let fn_names: Vec<&str> = symbols
        .iter()
        .flat_map(|fs| fs.symbols.iter())
        .filter(|s| s.kind == SymbolKind::Function)
        .map(|s| s.name.as_str())
        .collect();
    assert!(fn_names.contains(&"brand_new"));
    assert!(fn_names.contains(&"existing"));
}

#[test]
fn incremental_index_handles_deleted_file() {
    let tmp = TempDir::new().unwrap();
    make_project(
        &tmp,
        &[
            ("src/a.rs", "pub fn a() {}"),
            ("src/to_delete.rs", "pub fn gone() {}"),
        ],
    );

    let opts = IndexOptions {
        root: tmp.path().to_path_buf(),
        index_dir: index_dir(&tmp),
        incremental: true,
        verbose: false,
    };

    run_index(&opts).unwrap();

    // Delete one file.
    fs::remove_file(tmp.path().join("src/to_delete.rs")).unwrap();

    let (symbols, summary) = run_index(&opts).unwrap();
    assert_eq!(summary.total_files, 1);
    assert_eq!(summary.removed_files, 1);

    let fn_names: Vec<&str> = symbols
        .iter()
        .flat_map(|fs| fs.symbols.iter())
        .filter(|s| s.kind == SymbolKind::Function)
        .map(|s| s.name.as_str())
        .collect();
    assert!(!fn_names.contains(&"gone"), "deleted file's symbols should be absent");
    assert!(fn_names.contains(&"a"));
}

// ─── HashState unit tests ─────────────────────────────────────────────────────
// (Additional state tests beyond those in incremental.rs)

#[test]
fn hash_state_load_returns_empty_when_no_file() {
    let tmp = TempDir::new().unwrap();
    let state = HashState::load(tmp.path()).unwrap();
    assert!(state.hashes.is_empty());
}

// ─── Graph integration tests ──────────────────────────────────────────────────

#[test]
fn index_populates_graph_directory() {
    let tmp = TempDir::new().unwrap();
    make_project(&tmp, &[("src/lib.rs", "pub fn helper() {}")]);

    let opts = IndexOptions {
        root: tmp.path().to_path_buf(),
        index_dir: index_dir(&tmp),
        incremental: false,
        verbose: false,
    };

    let (_, summary) = run_index(&opts).unwrap();

    // Graph files should be created.
    assert!(
        index_dir(&tmp).join("graph").join("nodes.json").exists(),
        "nodes.json should be created"
    );
    assert!(
        index_dir(&tmp).join("graph").join("edges.json").exists(),
        "edges.json should be created"
    );

    // Summary should report graph counts.
    assert!(summary.graph_nodes >= 2, "at least File + Function nodes");
    assert!(summary.graph_edges >= 1, "at least one DEFINES edge");
}

#[test]
fn graph_node_count_reported_in_summary() {
    let tmp = TempDir::new().unwrap();
    make_project(
        &tmp,
        &[
            ("src/a.rs", "pub fn foo() {}\npub fn bar() {}"),
            ("src/b.rs", "pub struct Config {}"),
        ],
    );

    let opts = IndexOptions {
        root: tmp.path().to_path_buf(),
        index_dir: index_dir(&tmp),
        incremental: false,
        verbose: false,
    };

    let (_, summary) = run_index(&opts).unwrap();

    // 2 File nodes + 2 Function nodes + 1 Struct node = 5 minimum
    assert!(summary.graph_nodes >= 5, "expected ≥5 nodes, got {}", summary.graph_nodes);
    // At least 3 DEFINES edges (one per symbol across both files)
    assert!(summary.graph_edges >= 3, "expected ≥3 edges, got {}", summary.graph_edges);
}

#[test]
fn incremental_index_purges_stale_graph_nodes() {
    let tmp = TempDir::new().unwrap();
    make_project(
        &tmp,
        &[
            ("src/keep.rs", "pub fn kept() {}"),
            ("src/remove.rs", "pub fn gone() {}"),
        ],
    );

    let opts = IndexOptions {
        root: tmp.path().to_path_buf(),
        index_dir: index_dir(&tmp),
        incremental: true,
        verbose: false,
    };

    // First run populates the graph.
    let (_, summary1) = run_index(&opts).unwrap();
    let nodes_after_first = summary1.graph_nodes;

    // Delete one file and re-index.
    fs::remove_file(tmp.path().join("src/remove.rs")).unwrap();
    let (_, summary2) = run_index(&opts).unwrap();

    // Graph should have fewer nodes after the stale file is purged.
    assert!(
        summary2.graph_nodes < nodes_after_first,
        "graph should shrink after file deletion"
    );
}

#[test]
fn graph_persists_across_runs() {
    use ast_line::graph::{store::AdjacencyStore, GraphStore};

    let tmp = TempDir::new().unwrap();
    make_project(&tmp, &[("src/lib.rs", "pub fn entry() {}")]);

    let opts = IndexOptions {
        root: tmp.path().to_path_buf(),
        index_dir: index_dir(&tmp),
        incremental: true,
        verbose: false,
    };

    run_index(&opts).unwrap();

    // Load the persisted graph directly.
    let store = AdjacencyStore::load(&index_dir(&tmp)).unwrap();
    assert!(store.node_count() >= 2, "at least File + Function node");
    assert!(store.edge_count() >= 1, "at least one DEFINES edge");
}

// ─── CALLS edge tests ─────────────────────────────────────────────────────────

#[test]
fn parser_extracts_direct_function_calls() {
    let src = r#"
fn helper() {}

fn caller() {
    helper();
}
"#;
    let hash = fingerprint(src.as_bytes());
    let result = parse_file("test.rs", src, hash).unwrap();

    assert!(
        !result.calls.is_empty(),
        "expected at least one call site, found none"
    );
    let call = result.calls.iter().find(|c| c.callee_name == "helper");
    assert!(call.is_some(), "expected a call to 'helper'");
    let call = call.unwrap();
    assert_eq!(call.caller_name, "caller", "enclosing function should be 'caller'");
}

#[test]
fn parser_extracts_method_calls() {
    let src = r#"
fn process(v: Vec<u8>) -> usize {
    v.len()
}
"#;
    let hash = fingerprint(src.as_bytes());
    let result = parse_file("test.rs", src, hash).unwrap();

    let call = result.calls.iter().find(|c| c.callee_name == "len");
    assert!(call.is_some(), "expected a call to 'len'");
    assert_eq!(call.unwrap().caller_name, "process");
}

#[test]
fn parser_extracts_scoped_calls() {
    let src = r#"
fn setup() {
    let s = String::new();
    let _ = s;
}
"#;
    let hash = fingerprint(src.as_bytes());
    let result = parse_file("test.rs", src, hash).unwrap();

    let call = result.calls.iter().find(|c| c.callee_name == "new");
    assert!(call.is_some(), "expected a scoped call to 'new'");
    assert_eq!(call.unwrap().caller_name, "setup");
}

#[test]
fn indexer_emits_calls_edge_for_intra_file_call() {
    use ast_line::graph::{store::AdjacencyStore, GraphStore};

    let tmp = TempDir::new().unwrap();
    make_project(
        &tmp,
        &[(
            "src/lib.rs",
            r#"
pub fn helper() {}

pub fn caller() {
    helper();
}
"#,
        )],
    );

    let opts = IndexOptions {
        root: tmp.path().to_path_buf(),
        index_dir: index_dir(&tmp),
        incremental: false,
        verbose: false,
    };

    let (_, summary) = run_index(&opts).unwrap();

    // Load the persisted graph and verify a CALLS edge exists.
    let store = AdjacencyStore::load(&index_dir(&tmp)).unwrap();

    let edges: Vec<_> = store
        .edges()
        .filter(|e| e.edge_type == ast_line::graph::EdgeType::Calls)
        .collect();

    assert!(
        !edges.is_empty(),
        "expected at least one CALLS edge, got {summary:?}"
    );

    // The caller → helper edge should exist with full confidence.
    let call_edge = edges
        .iter()
        .find(|e| e.source_id.contains("caller") && e.target_id.contains("helper"));
    assert!(call_edge.is_some(), "expected a CALLS edge from 'caller' to 'helper'");
    assert_eq!(call_edge.unwrap().confidence, 1.0, "intra-file exact call should have confidence 1.0");
}

#[test]
fn indexer_emits_calls_edge_for_cross_file_call() {
    use ast_line::graph::{store::AdjacencyStore, GraphStore};

    let tmp = TempDir::new().unwrap();
    make_project(
        &tmp,
        &[
            ("src/lib.rs", "pub fn util() {}"),
            (
                "src/main.rs",
                r#"
fn main() {
    util();
}
"#,
            ),
        ],
    );

    let opts = IndexOptions {
        root: tmp.path().to_path_buf(),
        index_dir: index_dir(&tmp),
        incremental: false,
        verbose: false,
    };

    run_index(&opts).unwrap();

    let store = AdjacencyStore::load(&index_dir(&tmp)).unwrap();
    let calls: Vec<_> = store
        .edges()
        .filter(|e| e.edge_type == ast_line::graph::EdgeType::Calls)
        .collect();

    let cross = calls
        .iter()
        .find(|e| e.source_id.contains("main") && e.target_id.contains("util"));
    assert!(
        cross.is_some(),
        "expected a cross-file CALLS edge from 'main' to 'util'"
    );
}

#[test]
fn indexer_confidence_is_below_one_for_ambiguous_calls() {
    use ast_line::graph::{store::AdjacencyStore, GraphStore};

    // Two files both define a function called `helper`. A call to `helper`
    // from a third file is ambiguous → confidence < 1.0.
    let tmp = TempDir::new().unwrap();
    make_project(
        &tmp,
        &[
            ("src/a.rs", "pub fn helper() {}"),
            ("src/b.rs", "pub fn helper() {}"),
            (
                "src/main.rs",
                r#"
fn caller() {
    helper();
}
"#,
            ),
        ],
    );

    let opts = IndexOptions {
        root: tmp.path().to_path_buf(),
        index_dir: index_dir(&tmp),
        incremental: false,
        verbose: false,
    };

    run_index(&opts).unwrap();

    let store = AdjacencyStore::load(&index_dir(&tmp)).unwrap();
    let ambiguous: Vec<_> = store
        .edges()
        .filter(|e| {
            e.edge_type == ast_line::graph::EdgeType::Calls
                && e.source_id.contains("caller")
                && e.confidence < 1.0
        })
        .collect();

    assert!(
        !ambiguous.is_empty(),
        "expected ambiguous CALLS edges when multiple callees share the same name"
    );
    for edge in &ambiguous {
        assert!(
            edge.confidence < 1.0,
            "ambiguous edge confidence should be less than 1.0, got {}",
            edge.confidence
        );
    }
}
