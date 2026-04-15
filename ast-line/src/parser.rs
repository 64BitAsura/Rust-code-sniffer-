//! Tree-sitter–based Rust symbol extractor.
//!
//! Parses a single Rust source file and returns every top-level and
//! nested symbol (functions, structs, enums, traits, impl blocks, etc.)
//! together with every call site found in the file.

use tree_sitter::{Node, Parser};

use crate::error::SnifferError;
use crate::symbols::{FileSymbols, RouteAnnotation, Symbol, SymbolKind, UnresolvedAccess, UnresolvedCall, UnresolvedImport, Visibility};

/// Parse a Rust source file and extract all symbols.
///
/// `path` is stored verbatim in the returned [`FileSymbols`].
/// `hash` should be the pre-computed SHA-256 fingerprint of the content.
pub fn parse_file(path: &str, source: &str, hash: String) -> Result<FileSymbols, SnifferError> {
    let mut parser = Parser::new();
    let language = tree_sitter_rust::LANGUAGE;
    parser
        .set_language(&language.into())
        .map_err(|e| SnifferError::Parse(format!("failed to set language: {e}")))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| SnifferError::Parse(format!("tree-sitter returned None for {path}")))?;

    let root = tree.root_node();
    let mut symbols = Vec::new();
    extract_node(&root, source.as_bytes(), &mut symbols);

    // Second pass: extract call sites now that we have all symbol line ranges.
    let calls = extract_calls(&root, source.as_bytes(), &symbols);

    // Third pass: extract `use` import declarations.
    let imports = extract_imports(&root, source.as_bytes());

    // Fourth pass: extract field accesses.
    let accesses = extract_accesses(&root, source.as_bytes(), &symbols);

    // Fifth pass: extract HTTP route annotations.
    let routes = extract_routes(&root, source.as_bytes(), &symbols);

    Ok(FileSymbols {
        path: path.to_owned(),
        hash,
        symbols,
        calls,
        imports,
        accesses,
        routes,
    })
}

// ─── Internal extraction helpers ──────────────────────────────────────────────

/// Recursively visit `node` and push any recognised symbols into `out`.
fn extract_node(node: &Node<'_>, src: &[u8], out: &mut Vec<Symbol>) {
    match node.kind() {
        "function_item" => {
            if let Some(sym) = extract_function(node, src) {
                out.push(sym);
            }
            // Recurse into the block body for nested items.
            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i as u32) {
                    if child.kind() == "block" {
                        extract_block(&child, src, out);
                    }
                }
            }
            return;
        }
        "struct_item" => {
            if let Some(sym) = extract_named_item(node, src, SymbolKind::Struct) {
                out.push(sym);
                // Extract struct fields after the struct symbol.
                extract_struct_fields(node, src, out);
            }
        }
        "enum_item" => {
            if let Some(sym) = extract_named_item(node, src, SymbolKind::Enum) {
                out.push(sym);
            }
        }
        "trait_item" => {
            if let Some(sym) = extract_named_item(node, src, SymbolKind::Trait) {
                out.push(sym);
            }
            // Recurse into the trait body for associated functions.
            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i as u32) {
                    if child.kind() == "declaration_list" {
                        extract_declaration_list(&child, src, out);
                    }
                }
            }
            return;
        }
        "impl_item" => {
            extract_impl(node, src, out);
            return;
        }
        "mod_item" => {
            if let Some(sym) = extract_named_item(node, src, SymbolKind::Module) {
                out.push(sym);
            }
            // Recurse into inline module bodies.
            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i as u32) {
                    if child.kind() == "declaration_list" {
                        extract_declaration_list(&child, src, out);
                    }
                }
            }
            return;
        }
        "type_item" => {
            if let Some(sym) = extract_named_item(node, src, SymbolKind::TypeAlias) {
                out.push(sym);
            }
        }
        "const_item" => {
            if let Some(sym) = extract_named_item(node, src, SymbolKind::Constant) {
                out.push(sym);
            }
        }
        "static_item" => {
            if let Some(sym) = extract_named_item(node, src, SymbolKind::Static) {
                out.push(sym);
            }
        }
        "macro_definition" => {
            if let Some(sym) = extract_identifier_item(node, src, SymbolKind::Macro) {
                out.push(sym);
            }
        }
        // Trait method signatures inside a trait body.
        "function_signature_item" => {
            if let Some(sym) = extract_function(node, src) {
                out.push(sym);
            }
        }
        _ => {}
    }

    // Default: recurse into all named children.
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            extract_node(&child, src, out);
        }
    }
}

/// Recurse through a `block` node for nested functions/items.
fn extract_block(node: &Node<'_>, src: &[u8], out: &mut Vec<Symbol>) {
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            extract_node(&child, src, out);
        }
    }
}

/// Recurse through a `declaration_list` (module or trait body).
fn extract_declaration_list(node: &Node<'_>, src: &[u8], out: &mut Vec<Symbol>) {
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            extract_node(&child, src, out);
        }
    }
}

// ─── Specific extractors ──────────────────────────────────────────────────────

fn extract_function(node: &Node<'_>, src: &[u8]) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, src);
    let vis = extract_visibility(node, src);

    let start_line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;

    let mut sym = Symbol::new(name, SymbolKind::Function, vis, start_line, end_line);

    // Detect `async`
    sym.is_async = has_keyword_child(node, "async");

    // Return type (best-effort): the `return_type` field holds the type after `->`.
    if let Some(ret) = node.child_by_field_name("return_type") {
        sym.return_type = Some(node_text(&ret, src));
    }

    Some(sym)
}

fn extract_named_item(node: &Node<'_>, src: &[u8], kind: SymbolKind) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, src);
    let vis = extract_visibility(node, src);
    let start_line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;
    Some(Symbol::new(name, kind, vis, start_line, end_line))
}

/// For `macro_definition`, the name is an `identifier` child (no named `name` field).
fn extract_identifier_item(node: &Node<'_>, src: &[u8], kind: SymbolKind) -> Option<Symbol> {
    // First try the standard `name` field.
    if let Some(sym) = extract_named_item(node, src, kind.clone()) {
        return Some(sym);
    }
    // Fallback: first identifier child.
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            if child.kind() == "identifier" {
                let name = node_text(&child, src);
                let start_line = node.start_position().row + 1;
                let end_line = node.end_position().row + 1;
                return Some(Symbol::new(name, kind, Visibility::Private, start_line, end_line));
            }
        }
    }
    None
}

fn extract_impl(node: &Node<'_>, src: &[u8], out: &mut Vec<Symbol>) {
    let Some(type_node) = node.child_by_field_name("type") else {
        return;
    };
    let type_name = node_text(&type_node, src);

    let trait_node = node.child_by_field_name("trait");
    let (kind, trait_name) = if let Some(t) = trait_node {
        (SymbolKind::TraitImpl, Some(node_text(&t, src)))
    } else {
        (SymbolKind::Impl, None)
    };

    let start_line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;
    let vis = extract_visibility(node, src);

    let mut sym = Symbol::new(type_name, kind, vis, start_line, end_line);
    sym.trait_name = trait_name;
    out.push(sym);

    // Recurse into impl body for associated functions / methods.
    if let Some(body) = node.child_by_field_name("body") {
        for i in 0..body.named_child_count() {
            if let Some(child) = body.named_child(i as u32) {
                if child.kind() == "function_item" || child.kind() == "function_signature_item" {
                    if let Some(fn_sym) = extract_function(&child, src) {
                        out.push(fn_sym);
                    }
                }
            }
        }
    }
}

fn extract_struct_fields(node: &Node<'_>, src: &[u8], out: &mut Vec<Symbol>) {
    for i in 0..node.named_child_count() {
        let Some(child) = node.named_child(i as u32) else {
            continue;
        };
        if child.kind() == "field_declaration_list" {
            for j in 0..child.named_child_count() {
                let Some(field) = child.named_child(j as u32) else {
                    continue;
                };
                if field.kind() == "field_declaration" {
                    if let Some(sym) = extract_field(&field, src) {
                        out.push(sym);
                    }
                }
            }
            break;
        }
    }
}

fn extract_field(node: &Node<'_>, src: &[u8]) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, src);
    let vis = extract_visibility(node, src);
    let start_line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;

    let mut sym = Symbol::new(name, SymbolKind::Field, vis, start_line, end_line);

    if let Some(type_node) = node.child_by_field_name("type") {
        sym.field_type = Some(node_text(&type_node, src));
    }

    Some(sym)
}

// ─── Utilities ────────────────────────────────────────────────────────────────

fn node_text(node: &Node<'_>, src: &[u8]) -> String {
    node.utf8_text(src).unwrap_or("").trim().to_owned()
}

fn extract_visibility(node: &Node<'_>, src: &[u8]) -> Visibility {
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            if child.kind() == "visibility_modifier" {
                let text = node_text(&child, src);
                if text.starts_with("pub(") {
                    return Visibility::Restricted;
                }
                return Visibility::Public;
            }
        }
    }
    Visibility::Private
}

fn has_keyword_child(node: &Node<'_>, keyword: &str) -> bool {
    // Direct keyword child (e.g. `async` as a bare token)
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == keyword {
                return true;
            }
        }
    }
    // `async fn` wraps the modifier in a `function_modifiers` named node.
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            if child.kind() == "function_modifiers" {
                for j in 0..child.child_count() {
                    if let Some(mod_child) = child.child(j as u32) {
                        if mod_child.kind() == keyword {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

// ─── Call-site extraction ─────────────────────────────────────────────────────

/// Walk the AST and collect all call sites as [`UnresolvedCall`] values.
///
/// `symbols` must already contain the extracted symbols for this file so that
/// we can determine which function encloses each call site.
pub(crate) fn extract_calls(root: &Node<'_>, src: &[u8], symbols: &[Symbol]) -> Vec<UnresolvedCall> {
    let mut calls = Vec::new();
    walk_calls(root, src, symbols, &mut calls);
    calls
}

/// Recursively walk the subtree rooted at `node`, collecting call sites.
fn walk_calls(node: &Node<'_>, src: &[u8], symbols: &[Symbol], out: &mut Vec<UnresolvedCall>) {
    match node.kind() {
        "call_expression" => {
            if let Some(callee_name) = call_expression_callee(node, src) {
                let line = node.start_position().row + 1;
                let caller_name = enclosing_function(line, symbols)
                    .unwrap_or("")
                    .to_owned();
                out.push(UnresolvedCall { caller_name, callee_name, line });
            }
        }
        "method_call_expression" => {
            if let Some(callee_name) = method_call_expression_callee(node, src) {
                let line = node.start_position().row + 1;
                let caller_name = enclosing_function(line, symbols)
                    .unwrap_or("")
                    .to_owned();
                out.push(UnresolvedCall { caller_name, callee_name, line });
            }
        }
        _ => {}
    }

    // Recurse into all named children regardless of the current node kind so
    // that nested calls (e.g. `foo(bar())`) are also captured.
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            walk_calls(&child, src, symbols, out);
        }
    }
}

/// Extract the callee name from a `call_expression` node.
///
/// Handles the common patterns in Rust:
/// * `foo()`              → identifier             → `"foo"`
/// * `obj.field()`        → field_expression       → `"field"`
/// * `Struct::method()`   → scoped_identifier      → `"method"`
/// * `foo::<T>()`         → generic_function       → `"foo"`
fn call_expression_callee(node: &Node<'_>, src: &[u8]) -> Option<String> {
    let func = node.child_by_field_name("function")?;
    callee_from_node(&func, src)
}

/// Extract the callee (method) name from a `method_call_expression` node.
///
/// tree-sitter-rust places the method name in the `name` field.
fn method_call_expression_callee(node: &Node<'_>, src: &[u8]) -> Option<String> {
    node.child_by_field_name("name").map(|n| node_text(&n, src))
}

/// Derive the callee name from a function-position node.
fn callee_from_node(node: &Node<'_>, src: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" => Some(node_text(node, src)),
        "field_expression" => {
            // `(expr).field` — the method name is in the `field` child
            node.child_by_field_name("field").map(|f| node_text(&f, src))
        }
        "scoped_identifier" => {
            // `Path::name` — we want the leaf `name`
            node.child_by_field_name("name").map(|n| node_text(&n, src))
        }
        "generic_function" => {
            // `foo::<T>` — recurse into the inner function node
            let inner = node.child_by_field_name("function")?;
            callee_from_node(&inner, src)
        }
        _ => None,
    }
}

/// Find the name of the innermost `Function` symbol that contains `line`.
///
/// When multiple functions nest (closures aside), the last one in the sorted
/// symbol list whose range covers the line is the most specific enclosing
/// function.
fn enclosing_function(line: usize, symbols: &[Symbol]) -> Option<&str> {
    symbols
        .iter()
        .filter(|s| {
            matches!(s.kind, SymbolKind::Function)
                && s.start_line <= line
                && s.end_line >= line
        })
        // Pick the narrowest (most-nested) enclosing function.
        .min_by_key(|s| s.end_line - s.start_line)
        .map(|s| s.name.as_str())
}

// ─── Import extraction ────────────────────────────────────────────────────────

/// Walk the AST and collect all `use_declaration` paths as [`UnresolvedImport`] values.
///
/// Each `use` statement may produce one or more imports (e.g. `use a::{B, C}`
/// expands to two separate imports: `a::B` and `a::C`).
pub(crate) fn extract_imports(root: &Node<'_>, src: &[u8]) -> Vec<UnresolvedImport> {
    let mut imports = Vec::new();
    walk_imports(root, src, &mut imports);
    imports
}

/// Recursively walk the AST looking for `use_declaration` nodes.
fn walk_imports(node: &Node<'_>, src: &[u8], out: &mut Vec<UnresolvedImport>) {
    if node.kind() == "use_declaration" {
        let line = node.start_position().row + 1;
        let mut paths: Vec<(String, bool)> = Vec::new();
        if let Some(arg) = node.child_by_field_name("argument") {
            collect_use_paths(&arg, src, "", &mut paths);
        }
        for (raw_path, is_glob) in paths {
            out.push(UnresolvedImport { raw_path, is_glob, line });
        }
        // Do not recurse into `use_declaration` children — they are fully
        // consumed by `collect_use_paths`.
        return;
    }

    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            walk_imports(&child, src, out);
        }
    }
}

/// Recursively collect fully-qualified import path strings from a use-clause
/// node, expanding grouped imports (`use_list`, `scoped_use_list`) into
/// individual paths.
///
/// `prefix` accumulates the path built by enclosing `scoped_use_list` nodes
/// (e.g. `"crate::models"` when processing the inner list of
/// `use crate::models::{User, Repo}`).
fn collect_use_paths(node: &Node<'_>, src: &[u8], prefix: &str, out: &mut Vec<(String, bool)>) {
    match node.kind() {
        "identifier" => {
            let name = node_text(node, src);
            out.push((join_path(prefix, &name), false));
        }
        "scoped_identifier" => {
            // Full qualified path already encoded in the node text
            // (e.g. `crate::models::User`).
            let text = node_text(node, src);
            out.push((join_path(prefix, &text), false));
        }
        "use_wildcard" => {
            // `use crate::models::*` — the full text already includes the
            // leading path.  When nested inside a scoped_use_list the `*`
            // token appears as the only content, so we just append it.
            let text = node_text(node, src);
            let full = if text.starts_with("*") && !prefix.is_empty() {
                format!("{prefix}::*")
            } else {
                join_path(prefix, &text)
            };
            out.push((full, true));
        }
        "use_as_clause" => {
            // `use foo::Bar as Baz` — only the path before `as` is meaningful
            // for resolution.
            if let Some(path_node) = node.child_by_field_name("path") {
                collect_use_paths(&path_node, src, prefix, out);
            } else {
                // Fallback: strip the "as <alias>" suffix from the raw text.
                let text = node_text(node, src);
                let clean = text.split(" as ").next().unwrap_or(&text).trim().to_string();
                out.push((join_path(prefix, &clean), false));
            }
        }
        "use_list" => {
            // `{a, b, c}` — recurse into each named child.
            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i as u32) {
                    collect_use_paths(&child, src, prefix, out);
                }
            }
        }
        "scoped_use_list" => {
            // `crate::models::{User, Repo}` — build a new prefix from the
            // `path` field and recurse into the `list` field.
            let new_prefix = if let Some(path_node) = node.child_by_field_name("path") {
                join_path(prefix, &node_text(&path_node, src))
            } else {
                prefix.to_string()
            };
            if let Some(list_node) = node.child_by_field_name("list") {
                collect_use_paths(&list_node, src, &new_prefix, out);
            }
        }
        _ => {
            // Unknown or future node kind — best-effort: recurse into children.
            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i as u32) {
                    collect_use_paths(&child, src, prefix, out);
                }
            }
        }
    }
}

/// Concatenate `prefix` and `suffix` with `::` separator.
///
/// Returns `suffix` unchanged when `prefix` is empty.
fn join_path(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        suffix.to_string()
    } else {
        format!("{prefix}::{suffix}")
    }
}

// ─── Field access extraction ──────────────────────────────────────────────────

pub(crate) fn extract_accesses(root: &Node<'_>, src: &[u8], symbols: &[Symbol]) -> Vec<UnresolvedAccess> {
    let mut accesses = Vec::new();
    walk_accesses(root, src, symbols, &mut accesses);
    accesses
}

fn walk_accesses(node: &Node<'_>, src: &[u8], symbols: &[Symbol], out: &mut Vec<UnresolvedAccess>) {
    if node.kind() == "field_expression" {
        let line = node.start_position().row + 1;
        let accessor_fn = enclosing_function(line, symbols).unwrap_or("").to_owned();
        let child_count = node.named_child_count();
        if child_count > 0 {
            if let Some(field_node) = node.named_child((child_count - 1) as u32) {
                let field_name = node_text(&field_node, src);
                if !field_name.is_empty() && !accessor_fn.is_empty() {
                    let is_write = node.parent()
                        .map(|p| p.kind() == "assignment_expression" || p.kind() == "compound_assignment_expr")
                        .unwrap_or(false);
                    out.push(UnresolvedAccess {
                        accessor_fn,
                        field_name,
                        is_write,
                        line,
                    });
                }
            }
        }
    }
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            walk_accesses(&child, src, symbols, out);
        }
    }
}

// ─── HTTP route extraction ────────────────────────────────────────────────────

pub(crate) fn extract_routes(root: &Node<'_>, src: &[u8], symbols: &[Symbol]) -> Vec<RouteAnnotation> {
    let mut routes = Vec::new();
    walk_routes(root, src, symbols, &mut routes);
    routes
}

fn walk_routes(node: &Node<'_>, src: &[u8], symbols: &[Symbol], out: &mut Vec<RouteAnnotation>) {
    if node.kind() == "macro_invocation" {
        let line = node.start_position().row + 1;
        let macro_name = node.child_by_field_name("macro")
            .map(|n| node_text(&n, src))
            .unwrap_or_default();
        let http_methods = ["get", "post", "put", "patch", "delete", "options", "head"];
        if http_methods.contains(&macro_name.as_str()) {
            if let Some(args) = node.child_by_field_name("token_tree") {
                let raw = node_text(&args, src);
                let handler_fn = raw.trim_matches(|c| c == '(' || c == ')' || c == ' ').to_owned();
                if !handler_fn.is_empty() {
                    let handler = if handler_fn.contains(',') {
                        handler_fn.split(',').next().unwrap_or(&handler_fn).trim().to_owned()
                    } else {
                        handler_fn
                    };
                    let _enc = enclosing_function(line, symbols);
                    out.push(RouteAnnotation {
                        method: macro_name.to_uppercase(),
                        path: String::new(),
                        handler_fn: handler,
                        line,
                    });
                }
            }
        }
    }
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32) {
            walk_routes(&child, src, symbols, out);
        }
    }
}
