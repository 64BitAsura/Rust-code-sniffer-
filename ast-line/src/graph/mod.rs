//! Graph-database layer for symbol relationships.
//!
//! This module exposes a [`GraphStore`] trait and a lightweight, purely
//! in-process implementation ([`AdjacencyStore`]) that persists nodes and
//! edges as JSON files under `<index_dir>/graph/`.
//!
//! # Schema
//!
//! ## Node labels
//! `File`, `Function`, `Struct`, `Enum`, `Trait`, `Impl`, `Module`,
//! `TypeAlias`, `Constant`, `Static`, `Macro`, `Field`
//!
//! ## Edge types
//! `CALLS`, `IMPORTS`, `EXTENDS`, `IMPLEMENTS`, `HAS_METHOD`,
//! `HAS_PROPERTY`, `ACCESSES`, `METHOD_OVERRIDES`, `METHOD_IMPLEMENTS`,
//! `CONTAINS`, `DEFINES`, `MEMBER_OF`, `STEP_IN_PROCESS`, `HANDLES_ROUTE`

pub mod store;

pub use store::AdjacencyStore;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::SnifferError;

// ─── Node label ──────────────────────────────────────────────────────────────

/// The label (type) of a graph node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum NodeLabel {
    File,
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Module,
    TypeAlias,
    Constant,
    Static,
    Macro,
    Field,
}

impl std::fmt::Display for NodeLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            NodeLabel::File => "File",
            NodeLabel::Function => "Function",
            NodeLabel::Struct => "Struct",
            NodeLabel::Enum => "Enum",
            NodeLabel::Trait => "Trait",
            NodeLabel::Impl => "Impl",
            NodeLabel::Module => "Module",
            NodeLabel::TypeAlias => "TypeAlias",
            NodeLabel::Constant => "Constant",
            NodeLabel::Static => "Static",
            NodeLabel::Macro => "Macro",
            NodeLabel::Field => "Field",
        };
        write!(f, "{s}")
    }
}

// ─── Edge type ───────────────────────────────────────────────────────────────

/// The type of a directed relationship between two nodes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EdgeType {
    Calls,
    Imports,
    Extends,
    Implements,
    HasMethod,
    HasProperty,
    Accesses,
    MethodOverrides,
    MethodImplements,
    Contains,
    Defines,
    MemberOf,
    StepInProcess,
    HandlesRoute,
}

impl std::fmt::Display for EdgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            EdgeType::Calls => "CALLS",
            EdgeType::Imports => "IMPORTS",
            EdgeType::Extends => "EXTENDS",
            EdgeType::Implements => "IMPLEMENTS",
            EdgeType::HasMethod => "HAS_METHOD",
            EdgeType::HasProperty => "HAS_PROPERTY",
            EdgeType::Accesses => "ACCESSES",
            EdgeType::MethodOverrides => "METHOD_OVERRIDES",
            EdgeType::MethodImplements => "METHOD_IMPLEMENTS",
            EdgeType::Contains => "CONTAINS",
            EdgeType::Defines => "DEFINES",
            EdgeType::MemberOf => "MEMBER_OF",
            EdgeType::StepInProcess => "STEP_IN_PROCESS",
            EdgeType::HandlesRoute => "HANDLES_ROUTE",
        };
        write!(f, "{s}")
    }
}

// ─── Node and Edge structs ────────────────────────────────────────────────────

/// A node in the graph representing a code symbol or file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Unique identifier (e.g. `"file:src/main.rs"` or `"fn:src/main.rs::foo"`).
    pub id: String,
    /// The label (type) of this node.
    pub label: NodeLabel,
    /// Human-readable name (e.g. `"main"`, `"src/main.rs"`).
    pub name: String,
    /// The source file this node belongs to (empty for `File` nodes).
    pub file_path: String,
    /// 1-based start line in the source file (0 for `File` nodes).
    pub start_line: usize,
    /// 1-based end line in the source file (0 for `File` nodes).
    pub end_line: usize,
}

/// A directed edge between two nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    /// Unique identifier for this edge.
    pub id: String,
    /// ID of the source node.
    pub source_id: String,
    /// ID of the target node.
    pub target_id: String,
    /// The type of relationship.
    pub edge_type: EdgeType,
    /// Confidence score in [0.0, 1.0].
    pub confidence: f64,
    /// Optional human-readable reason for this relationship.
    pub reason: String,
}

// ─── GraphStore trait ─────────────────────────────────────────────────────────

/// Trait that all graph-store backends must implement.
///
/// This is the primary extension point for future query layers (e.g.
/// integrating a full KuzuDB engine or exporting to other graph formats).
pub trait GraphStore {
    /// Insert or update a node. Idempotent: inserting the same `id` again
    /// replaces the existing node.
    fn upsert_node(&mut self, node: Node);

    /// Insert or update an edge. Idempotent on `edge.id`.
    fn upsert_edge(&mut self, edge: Edge);

    /// Remove all nodes (and their incident edges) whose `file_path` matches
    /// the given path. Returns the number of nodes removed.
    fn remove_by_file(&mut self, file_path: &str) -> usize;

    /// Return the total number of nodes in the store.
    fn node_count(&self) -> usize;

    /// Return the total number of edges in the store.
    fn edge_count(&self) -> usize;

    /// Persist the current state to `<index_dir>/graph/`.
    fn save(&self, index_dir: &Path) -> Result<(), SnifferError>;

    /// Load state from `<index_dir>/graph/`, returning an empty store if the
    /// directory or files do not exist.
    fn load(index_dir: &Path) -> Result<Self, SnifferError>
    where
        Self: Sized;
}
