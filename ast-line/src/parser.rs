//! Tree-sitter–based Rust symbol extractor.
//!
//! Parses a single Rust source file and returns every top-level and
//! nested symbol (functions, structs, enums, traits, impl blocks, etc.)

use tree_sitter::{Node, Parser};

use crate::error::SnifferError;
use crate::symbols::{FileSymbols, Symbol, SymbolKind, Visibility};

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

    Ok(FileSymbols {
        path: path.to_owned(),
        hash,
        symbols,
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
