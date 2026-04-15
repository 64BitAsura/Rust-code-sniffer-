//! `ast-line` — Rust source-code indexer with incremental re-indexing and
//! graph-database support.
//!
//! # Library API
//!
//! The library exposes the following modules:
//!
//! * [`symbols`] — the data types returned by the indexer.
//! * [`parser`]  — single-file tree-sitter parser.
//! * [`incremental`] — SHA-256 fingerprinting and hash-state persistence.
//! * [`indexer`] — directory-walk orchestrator that wires everything together.
//! * [`graph`] — embedded graph database (node/edge types, [`GraphStore`] trait,
//!   and [`AdjacencyStore`] implementation persisted under `.ast-line/graph/`).
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use ast_line::indexer::{run_index, IndexOptions};
//! use std::path::PathBuf;
//!
//! let opts = IndexOptions {
//!     root: PathBuf::from("."),
//!     index_dir: PathBuf::from(".ast-line"),
//!     incremental: true,
//!     verbose: false,
//!     generate_embeddings: false,
//! };
//!
//! let (symbols, summary) = run_index(&opts).unwrap();
//! println!("{} symbols found in {} files", summary.total_symbols, summary.total_files);
//! ```

pub mod community;
pub mod embeddings;
pub mod error;
pub mod group;
pub mod graph;
pub mod incremental;
pub mod indexer;
pub mod mcp;
pub mod meta;
pub mod parser;
pub mod process;
pub mod registry;
pub mod search;
pub mod server;
pub mod symbols;
