//! Incremental indexing support.
//!
//! Persists a map of `{ file_path → sha256_fingerprint }` to
//! `<index_dir>/hashes.json`.  On subsequent runs only files whose
//! fingerprint has changed (or that are new) are re-parsed; unchanged
//! files are loaded directly from the cached symbol data.
//!
//! The fingerprint is the first 16 hex characters (64-bit prefix) of the
//! full SHA-256 digest — the same scheme used by the TypeScript GitNexus
//! pipeline (`RepoMeta.fileHashes`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use hex::encode as hex_encode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::SnifferError;

/// The file name that stores the hash state inside the index directory.
const HASHES_FILE: &str = "hashes.json";
/// The file name that stores the cached symbol data.
const SYMBOLS_FILE: &str = "symbols.json";

/// Persistent state written to / read from `<index_dir>/hashes.json`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct HashState {
    /// Map of canonical file path → 16-char SHA-256 prefix fingerprint.
    pub hashes: HashMap<String, String>,
}

impl HashState {
    /// Load state from `<index_dir>/hashes.json`.
    /// Returns an empty state if the file does not exist.
    pub fn load(index_dir: &Path) -> Result<Self, SnifferError> {
        let path = index_dir.join(HASHES_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)
            .map_err(|e| SnifferError::Io(format!("reading hashes.json: {e}")))?;
        serde_json::from_str(&raw)
            .map_err(|e| SnifferError::Json(format!("parsing hashes.json: {e}")))
    }

    /// Persist state to `<index_dir>/hashes.json`.
    pub fn save(&self, index_dir: &Path) -> Result<(), SnifferError> {
        fs::create_dir_all(index_dir)
            .map_err(|e| SnifferError::Io(format!("creating index dir: {e}")))?;
        let path = index_dir.join(HASHES_FILE);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SnifferError::Json(format!("serialising hashes: {e}")))?;
        fs::write(&path, json)
            .map_err(|e| SnifferError::Io(format!("writing hashes.json: {e}")))
    }

    /// Returns `true` when `file_path` is unchanged relative to the stored hash.
    pub fn is_unchanged(&self, file_path: &str, new_hash: &str) -> bool {
        self.hashes.get(file_path).map(|h| h == new_hash).unwrap_or(false)
    }

    /// Update the stored hash for `file_path`.
    pub fn update(&mut self, file_path: impl Into<String>, hash: impl Into<String>) {
        self.hashes.insert(file_path.into(), hash.into());
    }

    /// Remove a file that no longer exists in the workspace.
    pub fn remove(&mut self, file_path: &str) {
        self.hashes.remove(file_path);
    }

    /// Return paths that are recorded in the state but absent from `current_files`.
    pub fn stale_files<'a>(&'a self, current_files: &[String]) -> Vec<&'a str> {
        let set: std::collections::HashSet<&str> =
            current_files.iter().map(|s| s.as_str()).collect();
        self.hashes
            .keys()
            .filter(|k| !set.contains(k.as_str()))
            .map(|k| k.as_str())
            .collect()
    }
}

/// Compute a 16-char SHA-256 prefix fingerprint from raw bytes.
///
/// This mirrors the TypeScript implementation in `gitnexus/src/core/ingestion/file-hasher.ts`.
pub fn fingerprint(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let result = hasher.finalize();
    hex_encode(result)[..16].to_owned()
}

/// The result of comparing new files against the stored `HashState`.
#[derive(Debug)]
pub struct DiffResult {
    /// Files that are new or whose content has changed.
    pub changed: Vec<PathBuf>,
    /// Files that are unchanged (still need their cached symbols).
    pub unchanged: Vec<String>,
    /// Current fingerprints for all discovered files (key = canonical path).
    pub hashes: HashMap<String, String>,
}

/// Compare `files` against `state` and classify each file.
///
/// `files` is a list of `(canonical_path, file_content)` pairs.
pub fn diff_files(
    files: &[(String, Vec<u8>)],
    state: &HashState,
) -> DiffResult {
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();
    let mut hashes = HashMap::new();

    for (path, content) in files {
        let fp = fingerprint(content);
        if state.is_unchanged(path, &fp) {
            unchanged.push(path.clone());
        } else {
            changed.push(PathBuf::from(path));
        }
        hashes.insert(path.clone(), fp);
    }

    DiffResult { changed, unchanged, hashes }
}

/// Load the cached symbol JSON from a previous full run.
///
/// Returns `None` if the cache file does not exist or is unreadable.
pub fn load_cached_symbols(
    index_dir: &Path,
) -> Option<Vec<crate::symbols::FileSymbols>> {
    let path = index_dir.join(SYMBOLS_FILE);
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Persist the full symbol list to `<index_dir>/symbols.json`.
pub fn save_symbols(
    index_dir: &Path,
    symbols: &[crate::symbols::FileSymbols],
) -> Result<(), SnifferError> {
    fs::create_dir_all(index_dir)
        .map_err(|e| SnifferError::Io(format!("creating index dir: {e}")))?;
    let path = index_dir.join(SYMBOLS_FILE);
    let json = serde_json::to_string_pretty(symbols)
        .map_err(|e| SnifferError::Json(format!("serialising symbols: {e}")))?;
    fs::write(&path, json)
        .map_err(|e| SnifferError::Io(format!("writing symbols.json: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn fingerprint_is_16_hex_chars() {
        let fp = fingerprint(b"hello world");
        assert_eq!(fp.len(), 16);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let fp1 = fingerprint(b"content");
        let fp2 = fingerprint(b"content");
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn fingerprint_differs_on_content_change() {
        let fp1 = fingerprint(b"foo");
        let fp2 = fingerprint(b"bar");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn hash_state_round_trip() {
        let dir = TempDir::new().unwrap();
        let mut state = HashState::default();
        state.update("src/main.rs", "abcdef0123456789");
        state.save(dir.path()).unwrap();

        let loaded = HashState::load(dir.path()).unwrap();
        assert_eq!(
            loaded.hashes.get("src/main.rs").unwrap(),
            "abcdef0123456789"
        );
    }

    #[test]
    fn is_unchanged_detects_stale_hash() {
        let mut state = HashState::default();
        state.update("a.rs", "aaaaaaaaaaaaaaaa");
        assert!(state.is_unchanged("a.rs", "aaaaaaaaaaaaaaaa"));
        assert!(!state.is_unchanged("a.rs", "bbbbbbbbbbbbbbbb"));
        assert!(!state.is_unchanged("new.rs", "cccccccccccccccc"));
    }

    #[test]
    fn diff_files_classifies_correctly() {
        let mut state = HashState::default();
        let content_a = b"fn unchanged() {}".to_vec();
        let fp_a = fingerprint(&content_a);
        state.update("a.rs", &fp_a);

        let content_b = b"fn changed() {}".to_vec();
        let content_c = b"fn new_file() {}".to_vec();

        let files = vec![
            ("a.rs".to_owned(), content_a),
            ("b.rs".to_owned(), content_b),
            ("c.rs".to_owned(), content_c),
        ];

        let result = diff_files(&files, &state);
        assert_eq!(result.unchanged, vec!["a.rs".to_owned()]);
        assert_eq!(result.changed.len(), 2);
    }

    #[test]
    fn stale_files_returns_removed_paths() {
        let mut state = HashState::default();
        state.update("deleted.rs", "aaaaaaaaaaaaaaaa");
        state.update("kept.rs", "bbbbbbbbbbbbbbbb");

        let current = vec!["kept.rs".to_owned()];
        let stale = state.stale_files(&current);
        assert_eq!(stale, vec!["deleted.rs"]);
    }
}
