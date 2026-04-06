/**
 * Integration test: Rust AST decorator (attribute) detection
 *
 * Verifies that the RUST_QUERIES capture `#[get("/path")]`-style attributes
 * used by actix-web and rocket as decorator captures, enabling route extraction.
 */
import { describe, it, expect, beforeAll } from 'vitest';
import {
  loadParser,
  loadLanguage,
  isLanguageAvailable,
} from '../../src/core/tree-sitter/parser-loader.js';
import { SupportedLanguages } from '../../src/config/supported-languages.js';
import { getProvider } from '../../src/core/ingestion/languages/index.js';
import Parser from 'tree-sitter';

describe('Rust attribute decorator detection', () => {
  let parser: Parser;
  let query: Parser.Query;

  beforeAll(async () => {
    if (!isLanguageAvailable(SupportedLanguages.Rust)) return;

    parser = await loadParser();
    await loadLanguage(SupportedLanguages.Rust, 'test.rs');
    const provider = getProvider(SupportedLanguages.Rust);
    const grammar = parser.getLanguage();
    query = new Parser.Query(grammar, provider.treeSitterQueries);
  });

  const RUST_CODE = `
#[get("/users")]
pub async fn list_users() -> impl Responder { todo!() }

#[post("/users")]
pub async fn create_user() -> impl Responder { todo!() }

#[put("/users/{id}")]
pub async fn update_user() -> impl Responder { todo!() }

#[delete("/users/{id}")]
pub async fn delete_user() -> impl Responder { todo!() }

#[head("/healthz")]
pub async fn healthcheck() -> impl Responder { todo!() }

#[options("/api")]
pub async fn api_options() -> impl Responder { todo!() }
`;

  it('detects decorator name for #[get("/path")]', () => {
    if (!isLanguageAvailable(SupportedLanguages.Rust)) return;

    const tree = parser.parse(RUST_CODE);
    const matches = query.matches(tree.rootNode);

    const decoratorMatches = matches.filter((m) =>
      m.captures.some((c) => c.name === 'decorator'),
    );
    expect(decoratorMatches.length).toBeGreaterThan(0);

    const names = decoratorMatches.flatMap((m) =>
      m.captures.filter((c) => c.name === 'decorator.name').map((c) => c.node.text),
    );
    expect(names).toContain('get');
    expect(names).toContain('post');
    expect(names).toContain('put');
    expect(names).toContain('delete');
    expect(names).toContain('head');
    expect(names).toContain('options');
  });

  it('captures the route path as decorator.arg', () => {
    if (!isLanguageAvailable(SupportedLanguages.Rust)) return;

    const tree = parser.parse(RUST_CODE);
    const matches = query.matches(tree.rootNode);

    const routeArgs = matches.flatMap((m) =>
      m.captures.filter((c) => c.name === 'decorator.arg').map((c) => c.node.text),
    );
    expect(routeArgs).toContain('/users');
    expect(routeArgs).toContain('/users/{id}');
    expect(routeArgs).toContain('/healthz');
  });

  it('detects scoped attribute with string arg: #[actix_web::get("/path")]', () => {
    if (!isLanguageAvailable(SupportedLanguages.Rust)) return;

    const scopedCode = `
#[actix_web::get("/api/v2/users")]
pub async fn list_v2() -> impl Responder { todo!() }
`;
    const tree = parser.parse(scopedCode);
    const matches = query.matches(tree.rootNode);

    const decoratorMatches = matches.filter((m) =>
      m.captures.some((c) => c.name === 'decorator'),
    );
    expect(decoratorMatches.length).toBeGreaterThan(0);

    const names = decoratorMatches.flatMap((m) =>
      m.captures.filter((c) => c.name === 'decorator.name').map((c) => c.node.text),
    );
    expect(names).toContain('get');

    const args = decoratorMatches.flatMap((m) =>
      m.captures.filter((c) => c.name === 'decorator.arg').map((c) => c.node.text),
    );
    expect(args).toContain('/api/v2/users');
  });

  it('does not produce spurious decorator matches for #[derive(Debug)]', () => {
    if (!isLanguageAvailable(SupportedLanguages.Rust)) return;

    const deriveCode = `
#[derive(Debug, Clone, PartialEq)]
pub struct User { id: u64 }
`;
    const tree = parser.parse(deriveCode);
    const matches = query.matches(tree.rootNode);

    // derive has no string_literal arg, so it must NOT match as a @decorator.arg capture
    const argCaptures = matches.flatMap((m) =>
      m.captures.filter((c) => c.name === 'decorator.arg'),
    );
    expect(argCaptures).toHaveLength(0);
  });
});
