//! Symbol types extracted from Rust source files.

use serde::{Deserialize, Serialize};

/// The kind of a code symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    /// A concrete `impl` block (not a trait implementation).
    Impl,
    /// A `impl Trait for Type` block.
    TraitImpl,
    Module,
    TypeAlias,
    Constant,
    Static,
    Macro,
    /// A named field inside a struct or enum variant.
    Field,
}

/// Visibility of a symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    /// `pub(crate)` or `pub(super)` — restricted public.
    Restricted,
    Private,
}

/// A single code symbol extracted from a Rust source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// The name of the symbol (e.g. `"my_function"`).
    pub name: String,
    /// The kind of the symbol.
    pub kind: SymbolKind,
    /// Visibility of the symbol.
    pub visibility: Visibility,
    /// 1-based start line in the source file.
    pub start_line: usize,
    /// 1-based end line in the source file.
    pub end_line: usize,
    /// For `TraitImpl`, the name of the implemented trait.
    pub trait_name: Option<String>,
    /// For `Field`, the declared type as a string (best-effort).
    pub field_type: Option<String>,
    /// For `Function`, the return type as a string (best-effort).
    pub return_type: Option<String>,
    /// `true` when the function signature carries `async`.
    pub is_async: bool,
}

impl Symbol {
    /// Convenience constructor — callers fill optional fields afterwards.
    pub fn new(
        name: impl Into<String>,
        kind: SymbolKind,
        visibility: Visibility,
        start_line: usize,
        end_line: usize,
    ) -> Self {
        Symbol {
            name: name.into(),
            kind,
            visibility,
            start_line,
            end_line,
            trait_name: None,
            field_type: None,
            return_type: None,
            is_async: false,
        }
    }
}

/// An unresolved function/method call extracted from a source file.
///
/// The call is "unresolved" in the sense that we know the callee *name* but
/// not yet the UID of the target node.  Resolution against the global symbol
/// table happens in the indexer after all files have been parsed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedCall {
    /// Name of the enclosing function (caller).  Empty string when the call
    /// site is not inside any recognised function.
    pub caller_name: String,
    /// Best-effort callee name extracted from the AST (e.g. `"new"`, `"push"`).
    pub callee_name: String,
    /// 1-based line number of the call site.
    pub line: usize,
}

/// An unresolved `use` import extracted from a source file.
///
/// The import is "unresolved" in the sense that we have extracted the raw
/// path text from the AST but have not yet mapped it to a file node.
/// Resolution against the global file list happens in the indexer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedImport {
    /// The canonical import path (e.g. `"crate::models::User"`,
    /// `"super::utils"`, `"crate::foo::*"`).
    pub raw_path: String,
    /// 1-based line number of the `use` declaration.
    pub line: usize,
    /// `true` when the import uses a glob wildcard (`::*`).
    pub is_glob: bool,
}

/// An unresolved struct/field access extracted from a source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedAccess {
    /// Name of the enclosing function performing the access.
    pub accessor_fn: String,
    /// Field name being accessed.
    pub field_name: String,
    /// `true` when the access is a write (assignment).
    pub is_write: bool,
    /// 1-based line number of the access.
    pub line: usize,
}

/// An HTTP route annotation extracted from axum/actix-style handler macros.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteAnnotation {
    /// HTTP method (GET, POST, etc.).
    pub method: String,
    /// Route path pattern (may be empty if not determinable statically).
    pub path: String,
    /// Name of the handler function.
    pub handler_fn: String,
    /// 1-based line number of the route registration.
    pub line: usize,
}

/// All symbols extracted from a single source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSymbols {
    /// Absolute or repo-relative path of the file.
    pub path: String,
    /// SHA-256 fingerprint (first 16 hex chars) of the file content at index time.
    pub hash: String,
    /// Extracted symbols, in source order.
    pub symbols: Vec<Symbol>,
    /// Unresolved call sites extracted from this file.
    pub calls: Vec<UnresolvedCall>,
    /// Unresolved import paths from `use` declarations in this file.
    #[serde(default)]
    pub imports: Vec<UnresolvedImport>,
    /// Unresolved field accesses extracted from this file.
    #[serde(default)]
    pub accesses: Vec<UnresolvedAccess>,
    /// HTTP route annotations extracted from this file.
    #[serde(default)]
    pub routes: Vec<RouteAnnotation>,
}
