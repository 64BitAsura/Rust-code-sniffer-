//! `rust-sniffer` — Rust source-code indexer with incremental re-indexing.
//!
//! # Library API
//!
//! The library exposes three main modules:
//!
//! * [`symbols`] — the data types returned by the indexer.
//! * [`parser`]  — single-file tree-sitter parser.
//! * [`incremental`] — SHA-256 fingerprinting and hash-state persistence.
//! * [`indexer`] — directory-walk orchestrator that wires everything together.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use rust_sniffer::indexer::{run_index, IndexOptions};
//! use std::path::PathBuf;
//!
//! let opts = IndexOptions {
//!     root: PathBuf::from("."),
//!     index_dir: PathBuf::from(".rust-sniffer"),
//!     incremental: true,
//!     verbose: false,
//! };
//!
//! let (symbols, summary) = run_index(&opts).unwrap();
//! println!("{} symbols found in {} files", summary.total_symbols, summary.total_files);
//! ```

pub mod error;
pub mod incremental;
pub mod indexer;
pub mod parser;
pub mod symbols;
