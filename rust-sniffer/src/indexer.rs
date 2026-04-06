//! Directory-walking indexer that wires the parser and incremental state together.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::error::SnifferError;
use crate::incremental::{
    diff_files, fingerprint, load_cached_symbols, save_symbols, HashState,
};
use crate::meta::IndexMeta;
use crate::parser::parse_file;
use crate::symbols::FileSymbols;

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

    // ── 9. Always write meta.json so status / serve have fresh stats ──────────
    let root_str = opts.root.to_string_lossy().into_owned();
    let meta = IndexMeta::new(root_str, total_files, total_symbols);
    // Best-effort: a metadata write failure is not fatal.
    let _ = meta.save(&opts.index_dir);

    let summary = IndexSummary {
        total_files,
        parsed_files: parsed_count,
        skipped_files: skipped_count,
        total_symbols,
        removed_files: removed_count,
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
