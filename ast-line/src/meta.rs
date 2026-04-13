//! Index metadata persisted to `<index_dir>/meta.json`.
//!
//! Written after every successful `index` run so that `status` and `serve`
//! can report statistics without re-scanning the project.

use std::fs;
use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::SnifferError;

const META_FILE: &str = "meta.json";

/// Metadata written to `.ast-line/meta.json` after each successful index run.
/// Includes file count, symbol count, and graph node/edge counts so that the
/// `status` and `serve` commands can report statistics without re-scanning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    /// RFC 3339 timestamp of when the index was last built.
    pub indexed_at: String,
    /// The root directory that was indexed (absolute path).
    pub root: String,
    /// Number of source files in the index.
    pub file_count: usize,
    /// Total number of extracted symbols.
    pub symbol_count: usize,
    /// Number of nodes in the graph store.
    pub graph_node_count: usize,
    /// Number of edges in the graph store.
    pub graph_edge_count: usize,
}

impl IndexMeta {
    /// Create a new `IndexMeta` stamped with the current UTC time.
    pub fn new(
        root: impl Into<String>,
        file_count: usize,
        symbol_count: usize,
        graph_node_count: usize,
        graph_edge_count: usize,
    ) -> Self {
        IndexMeta {
            indexed_at: Utc::now().to_rfc3339(),
            root: root.into(),
            file_count,
            symbol_count,
            graph_node_count,
            graph_edge_count,
        }
    }

    /// Load from `<index_dir>/meta.json`.  Returns `None` if the file is
    /// absent or cannot be parsed.
    pub fn load(index_dir: &Path) -> Option<Self> {
        let path = index_dir.join(META_FILE);
        let raw = fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// Persist to `<index_dir>/meta.json`.
    pub fn save(&self, index_dir: &Path) -> Result<(), SnifferError> {
        fs::create_dir_all(index_dir)
            .map_err(|e| SnifferError::Io(format!("creating index dir: {e}")))?;
        let path = index_dir.join(META_FILE);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SnifferError::Json(format!("serialising meta: {e}")))?;
        fs::write(&path, json)
            .map_err(|e| SnifferError::Io(format!("writing meta.json: {e}")))
    }
}
