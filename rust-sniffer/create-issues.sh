#!/usr/bin/env bash
# create-issues.sh — Create all 25 GitNexus-parity issues for rust-sniffer.
#
# Prerequisites:
#   gh auth login   (GitHub CLI authenticated)
#   Run from any directory; the script targets the remote automatically.
#
# Usage:
#   chmod +x create-issues.sh
#   ./create-issues.sh
#
# Each issue is tagged with one or more labels that are created if they don't exist.
# Dependency cross-references are embedded in each issue body.

set -euo pipefail

REPO="64BitAsura/Rust-code-sniffer-"

# ── helpers ───────────────────────────────────────────────────────────────────

ensure_label() {
  local name="$1" color="$2" desc="$3"
  gh label create "$name" --color "$color" --description "$desc" --repo "$REPO" 2>/dev/null || true
}

new_issue() {
  local title="$1" body="$2"
  shift 2
  local labels=("$@")
  local label_args=()
  for l in "${labels[@]}"; do
    label_args+=(--label "$l")
  done
  gh issue create \
    --repo "$REPO" \
    --title "$title" \
    --body "$body" \
    "${label_args[@]}"
  echo "  Created: $title"
}

# ── labels ────────────────────────────────────────────────────────────────────

ensure_label "feat"            "0075ca" "New feature"
ensure_label "perf"            "e4e669" "Performance improvement"
ensure_label "p0-foundation"   "b60205" "Priority 0 — foundational"
ensure_label "p1-analysis"     "d93f0b" "Priority 1 — analysis layer"
ensure_label "p2-intelligence" "e99695" "Priority 2 — intelligence layer"
ensure_label "p3-mcp-core"     "0e8a16" "Priority 3 — MCP server core"
ensure_label "p4-mcp-extended" "006b75" "Priority 4 — MCP extended tools"
ensure_label "p5-multi-repo"   "1d76db" "Priority 5 — multi-repo"
ensure_label "p6-ai"           "5319e7" "Priority 6 — AI features"
ensure_label "graph"           "c2e0c6" "Knowledge graph"
ensure_label "mcp"             "fef2c0" "MCP / AI agent interface"
ensure_label "search"          "bfd4f2" "Search / ranking"
ensure_label "routes"          "d4c5f9" "HTTP route intelligence"

echo "Labels ready. Creating issues…"
echo ""

# ── Issue 1 ───────────────────────────────────────────────────────────────────

new_issue \
  "feat: embed a graph database for symbol relationships (KuzuDB or equivalent)" \
'## Summary
GitNexus stores every symbol and relationship in an embedded graph database
(LadybugDB wrapping KuzuDB). All subsequent analysis — call graphs, impact
analysis, community detection — query this graph.

## Work
- Add a Rust-native embedded graph DB (e.g. KuzuDB Rust bindings, or a compact
  custom adjacency-list store persisted as columnar files).
- Define node labels: `File`, `Function`, `Struct`, `Enum`, `Trait`, `Impl`,
  `Module`, `TypeAlias`, `Constant`, `Static`, `Macro`, `Field`.
- Define an edge table with `type` and `confidence` columns matching the
  GitNexus schema: `CALLS`, `IMPORTS`, `EXTENDS`, `IMPLEMENTS`, `HAS_METHOD`,
  `HAS_PROPERTY`, `ACCESSES`, `METHOD_OVERRIDES`, `METHOD_IMPLEMENTS`,
  `CONTAINS`, `DEFINES`, `MEMBER_OF`, `STEP_IN_PROCESS`, `HANDLES_ROUTE`.
- Migrate the existing flat `symbols.json` cache to populate the graph on index.
- Expose a `GraphStore` trait for future query layers.

## Acceptance criteria
- `rust-sniffer index` populates the graph DB in `.rust-sniffer/graph/`.
- Node and edge counts are reported in `status` output.
- Graph survives incremental re-index (stale nodes are purged).

**Depends on:** none (foundational — all other issues build on this)' \
  "feat" "p0-foundation" "graph"

# ── Issue 2 ───────────────────────────────────────────────────────────────────

new_issue \
  "feat: extract function call graph (CALLS edges) from Rust AST" \
'## Summary
GitNexus builds a CALLS relationship graph between every caller and callee.
For Rust this requires parsing call expressions from tree-sitter and resolving
the callee name to an indexed symbol.

## Work
- In `parser.rs`, walk `call_expression` and `method_call_expression` AST nodes.
- Extract the callee name (best-effort, no full type inference in MVP).
- Store unresolved CALLS edges with the caller symbol UID and callee name string.
- After all files are parsed, resolve callee names to UIDs via the symbol table;
  store `confidence = 1.0` for resolved, `< 0.8` for ambiguous matches.
- Write CALLS edges to the graph DB (see issue #1).

## Acceptance criteria
- `context` MCP tool (see issue #15) shows callers and callees.
- Confidence scores are present on each edge.

**Depends on:** #1' \
  "feat" "p0-foundation" "graph"

# ── Issue 3 ───────────────────────────────────────────────────────────────────

new_issue \
  "feat: parse \`use\` declarations and emit IMPORTS edges" \
'## Summary
GitNexus resolves import statements into IMPORTS edges linking the importing
file/symbol to the imported symbol, enabling transitive impact analysis.

## Work
- Walk `use_declaration` nodes in the Rust AST.
- Resolve each path to the canonical file it originates from (using the
  walkdir file list + module path mapping).
- Emit IMPORTS edges in the graph DB (issue #1).
- Handle glob imports (`use foo::*`) with lower confidence.

**Depends on:** #1' \
  "feat" "p0-foundation" "graph"

# ── Issue 4 ───────────────────────────────────────────────────────────────────

new_issue \
  "feat: emit IMPLEMENTS / METHOD_IMPLEMENTS edges for trait implementations" \
'## Summary
GitNexus emits IMPLEMENTS and METHOD_IMPLEMENTS edges so that impact analysis
can trace through trait hierarchies. rust-sniffer already identifies `TraitImpl`
symbols but does not yet emit graph edges.

## Work
- For each `TraitImpl` symbol, emit an IMPLEMENTS edge from the implementing
  type to the trait node in the graph DB (issue #1).
- For each method inside the impl body, emit METHOD_IMPLEMENTS linking the
  concrete method to the abstract method signature in the trait definition.

**Depends on:** #1, #2' \
  "feat" "p1-analysis" "graph"

# ── Issue 5 ───────────────────────────────────────────────────────────────────

new_issue \
  "feat: track struct field read/write accesses (ACCESSES edges with reason)" \
'## Summary
GitNexus emits ACCESSES edges with `reason: "read"` or `reason: "write"` so
that tools like `context` can answer "who writes this field?".

## Work
- Walk `field_expression` nodes inside function bodies.
- Determine read vs. write by inspecting whether the field expression appears
  as the left-hand side of an `assignment_expression` AST node.
- Emit ACCESSES edges in the graph DB (issue #1) linking the enclosing function
  to the target field symbol.

**Depends on:** #1, #2' \
  "feat" "p1-analysis" "graph"

# ── Issue 6 ───────────────────────────────────────────────────────────────────

new_issue \
  "feat: detect functional areas (communities) via Leiden algorithm" \
'## Summary
GitNexus groups symbols into Community nodes using the Leiden graph clustering
algorithm. Each community gets a heuristic label (e.g. "Auth", "Payments") and
a description. This powers the `clusters` resource and process-grouped query results.

## Work
- Implement or integrate a Leiden/Louvain community detection algorithm on the
  CALLS + IMPORTS subgraph.
- Assign every symbol a community UID.
- Persist Community nodes with: `heuristicLabel`, `cohesion`, `symbolCount`, `keywords`.
- Optionally call an LLM to enrich community descriptions (opt-in via `augment` command — see #24).
- Expose communities via `GET /api/communities` REST endpoint.

**Depends on:** #1, #2, #3' \
  "feat" "p2-intelligence" "graph"

# ── Issue 7 ───────────────────────────────────────────────────────────────────

new_issue \
  "feat: trace execution flows and persist Process nodes" \
'## Summary
GitNexus identifies "processes" — linear call chains from an entry point (e.g.
a public function, a route handler, `fn main`) to terminal functions. These appear
in `query` results and the `process/{name}` MCP resource.

## Work
- Score entry points: public functions, `fn main`, `#[actix_web::get]` handlers,
  etc. (see #8 for entry-point scoring).
- DFS/BFS from each entry point over CALLS edges up to a configurable max depth.
- Collapse each reachable path into a Process node with `stepCount`, `entryPointId`,
  `terminalId`, `communities`, `heuristicLabel`.
- Persist STEP_IN_PROCESS edges with a `step` ordinal.
- Expose via `GET /api/processes` REST endpoint and the
  `gitnexus://repo/{name}/processes` MCP resource.

**Depends on:** #1, #2, #6' \
  "feat" "p2-intelligence" "graph"

# ── Issue 8 ───────────────────────────────────────────────────────────────────

new_issue \
  "feat: score and rank entry-point symbols (public API surface, route handlers, main)" \
'## Summary
GitNexus ranks symbols as entry-point candidates based on visibility, being
unreachable from other nodes (in-degree = 0), and framework annotations.
This feeds Process detection (issue #7).

## Work
- After call graph construction, compute in-degree for every symbol.
- Symbols with in-degree = 0 that are `pub` or `pub(crate)` are primary candidates.
- Boost symbols annotated with `#[get(...)]`, `#[post(...)]`, `#[actix_web::get(...)]`,
  etc. (actix-web / rocket / axum route decorators).
- Persist `entryPointScore` on each symbol node.

**Depends on:** #1, #2' \
  "feat" "p1-analysis" "graph"

# ── Issue 9 ───────────────────────────────────────────────────────────────────

new_issue \
  "feat: extract HTTP route annotations and emit Route nodes (actix-web, rocket, axum)" \
'## Summary
GitNexus indexes HTTP API routes defined via framework macros/attributes and
emits Route nodes with path, method, and handler linkage. This powers the
`route_map`, `shape_check`, and `api_impact` MCP tools.

## Work
- Walk `attribute_item` nodes in the Rust AST for `#[get("/path")]`,
  `#[post("/path")]`, `#[actix_web::get(...)]`, `#[rocket::get(...)]`,
  `#[axum::debug_handler]` + `Router::route(...)` call patterns.
- Emit a Route node for each discovered route: `path`, `method`, `handlerSymbolId`.
- Emit HANDLES_ROUTE edges linking the handler function to its Route node.
- Expose via `GET /api/routes` REST endpoint.

**Depends on:** #1' \
  "feat" "p1-analysis" "routes"

# ── Issue 10 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: BM25 full-text search index over symbol names and metadata" \
'## Summary
GitNexus uses a BM25 index to enable keyword search over symbol names,
community labels, and process names. This is the fast path of the hybrid
search pipeline.

## Work
- Integrate a Rust BM25 crate (e.g. `bm25`) or implement the algorithm directly.
- Index fields: symbol name, kind, file path, community label, process label.
- Persist the index to `.rust-sniffer/bm25.bin`.
- Rebuild incrementally — re-index only symbols from changed files.
- Expose an internal `search_bm25(query, limit) -> Vec<ScoredSymbol>` API.

**Depends on:** #1' \
  "feat" "p1-analysis" "search"

# ── Issue 11 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: optional vector embeddings via HTTP embedding provider" \
'## Summary
GitNexus generates text descriptions of symbols, calls an OpenAI-compatible
HTTP embedding provider, and stores the resulting vectors for semantic search.
Embeddings are opt-in.

## Work
- Add `--embeddings` flag to `rust-sniffer index`.
- Read `EMBEDDING_API_URL` and `EMBEDDING_API_KEY` from the environment.
- Generate text for each symbol: `"{kind} {name} in {file}: {signature}"`.
- Batch-call the embedding API with configurable batch size.
- Persist embeddings to `.rust-sniffer/embeddings.bin` (e.g. HNSW index via
  `usearch` or `hora` crate).
- Track the embedding count in `meta.json` (`stats.embeddings` field).

**Depends on:** #1' \
  "feat" "p6-ai" "search"

# ── Issue 12 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: hybrid search via Reciprocal Rank Fusion (BM25 + semantic vector)" \
'## Summary
GitNexus merges BM25 and vector search results using Reciprocal Rank Fusion (RRF)
to produce a ranked list of symbols and processes. This is what the `query` MCP
tool uses.

## Work
- Implement RRF: for each result in each ranked list, compute
  `score += 1 / (k + rank)`, merge by symbol UID, sort by total score.
- When embeddings are unavailable, fall back to BM25-only.
- Rank processes by the maximum score of their member symbols.
- Return process-grouped output: ranked processes → symbols per process.

**Depends on:** #10, #11' \
  "feat" "p2-intelligence" "search"

# ── Issue 13 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: Model Context Protocol (MCP) server over stdio" \
'## Summary
GitNexus exposes its intelligence tools to AI agents via the MCP protocol over
stdio. This is the primary interface used by Claude, Cursor, and other agents.

## Work
- Implement MCP JSON-RPC 2.0 message framing over stdin/stdout.
- Handle protocol messages: `initialize`, `tools/list`, `tools/call`,
  `resources/list`, `resources/read`, `prompts/list`, `prompts/get`.
- Add `rust-sniffer mcp` CLI subcommand that starts the stdio server.
- Dispatch tool calls to handler modules (each MCP tool is its own handler).
- Expose resources:
  - `gitnexus://repo/{name}/context`
  - `gitnexus://repo/{name}/clusters`
  - `gitnexus://repo/{name}/processes`
  - `gitnexus://repo/{name}/process/{name}`
  - `gitnexus://repo/{name}/schema`

**Depends on:** #1' \
  "feat" "p3-mcp-core" "mcp"

# ── Issue 14 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: MCP tool \`query\` — hybrid search returning execution flows" \
'## Summary
The `query` tool is the primary discovery mechanism for AI agents. It searches
the knowledge graph and returns results grouped by execution flow (Process),
with symbols, file locations, and community membership.

## Work
- Implement the `query` tool handler dispatched from the MCP server (issue #13).
- Accept params: `query`, `task_context`, `goal`, `limit`, `max_symbols`,
  `include_content`, `repo`.
- Run hybrid search (issue #12) → ranked symbols.
- For each symbol, find its parent Process(es) via STEP_IN_PROCESS edges.
- Group output by process; include process rank, symbol list, and file paths.
- Include a `definitions` bucket for symbols not in any process.

**Depends on:** #12, #7, #13' \
  "feat" "p4-mcp-extended" "mcp" "search"

# ── Issue 15 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: MCP tool \`context\` — 360-degree view of a single symbol" \
'## Summary
The `context` tool gives AI agents a complete picture of a symbol: all incoming
and outgoing edges, process membership, source location, and optionally full
source code. It is the most commonly used tool for deep dives.

## Work
- Accept params: `name`, `uid`, `file_path`, `include_content`, `repo`.
- Resolve symbol by name (with disambiguation list if multiple matches) or by uid.
- Query the graph for all edges: CALLS, IMPORTS, EXTENDS, IMPLEMENTS, HAS_METHOD,
  HAS_PROPERTY, ACCESSES, METHOD_OVERRIDES.
- Categorize edges into labelled buckets: callers, callees, imports, extends,
  implementors, accesses (reads/writes).
- List process names where the symbol appears as a step.
- Optionally include raw source extracted from the file at the stored line range.

**Depends on:** #1, #2, #3, #4, #5, #13' \
  "feat" "p3-mcp-core" "mcp"

# ── Issue 16 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: MCP tool \`impact\` — upstream/downstream blast radius analysis" \
'## Summary
The `impact` tool traverses the call graph upstream or downstream from a target
symbol and produces a risk report (LOW / MEDIUM / HIGH / CRITICAL) with affected
symbols grouped by depth.

## Work
- BFS/DFS over graph edges (CALLS, IMPORTS, EXTENDS, IMPLEMENTS — configurable).
- Group results by traversal depth:
  - d=1 WILL BREAK (direct callers/importers)
  - d=2 LIKELY AFFECTED (indirect dependents)
  - d=3 MAY NEED TESTING (transitive)
- Compute risk level: CRITICAL (d=1 ≥ 10), HIGH (d=1 ≥ 5), MEDIUM (d=1 ≥ 2), LOW otherwise.
- Include: `affected_processes` (which flows break), `affected_modules` (communities).
- Accept params: `target`, `direction`, `maxDepth`, `relationTypes`, `includeTests`,
  `minConfidence`, `repo`.

**Depends on:** #1, #2, #6, #7, #13' \
  "feat" "p3-mcp-core" "mcp" "graph"

# ── Issue 17 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: MCP tool \`detect_changes\` — map git diff hunks to affected symbols and flows" \
'## Summary
`detect_changes` reads the current git diff (unstaged, staged, all, or compare
to a base ref), maps changed line ranges to indexed symbols, then traverses the
graph to find affected execution flows and produces a risk summary.

## Work
- Integrate `git2` crate (or shell out to `git diff`) to get changed line ranges
  per file.
- Map each changed line range to symbols that overlap that range in the index.
- Run impact analysis (issue #16) on each directly-changed symbol.
- Deduplicate and aggregate into affected processes + risk summary.
- Support scopes: `unstaged`, `staged`, `all`, `compare` (with `base_ref`).
- Accept params: `scope`, `base_ref`, `repo`.

**Depends on:** #16, #13' \
  "feat" "p4-mcp-extended" "mcp"

# ── Issue 18 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: MCP tool \`rename\` — graph-aware multi-file rename with dry-run preview" \
'## Summary
The `rename` tool uses the call graph to find all references to a symbol
(callers, imports, trait implementations) and produces a preview diff before
applying changes. Graph-based hits have high confidence; text-search hits have
lower confidence and require manual review.

## Work
- Graph phase: query CALLS, IMPORTS, IMPLEMENTS, METHOD_IMPLEMENTS edges for
  all referencing symbols; record file path + character offset.
- Text-search phase: regex scan for the symbol name across the codebase.
- Tag each edit: `"graph"` (high confidence) or `"text_search"` (review needed).
- `dry_run: true` (default) returns the edit list without writing files.
- `dry_run: false` applies the edits in-place.
- Accept params: `symbol_name`, `symbol_uid`, `new_name`, `file_path`, `dry_run`, `repo`.

**Depends on:** #15, #13' \
  "feat" "p4-mcp-extended" "mcp"

# ── Issue 19 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: MCP tool \`cypher\` — raw graph query execution" \
'## Summary
`cypher` lets advanced AI agents run arbitrary graph queries against the
knowledge graph. GitNexus uses a Cypher-like language via KuzuDB.
rust-sniffer should expose an equivalent query surface over its graph DB.

## Work
- Choose query language: Cypher (if using KuzuDB Rust bindings) or a custom DSL.
- Parse and execute queries against the graph DB (issue #1).
- Format results as a Markdown table (matching GitNexus output contract).
- Return: `{ markdown, row_count }`.
- Document the full schema (nodes, edges, properties) in the
  `gitnexus://repo/{name}/schema` MCP resource.
- Accept params: `query`, `repo`.

**Depends on:** #1, #13' \
  "feat" "p3-mcp-core" "mcp" "graph"

# ── Issue 20 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: MCP tools \`route_map\`, \`shape_check\`, and \`api_impact\` for HTTP route intelligence" \
'## Summary
Three related MCP tools built on top of Route nodes (issue #9):

- **`route_map`** — list routes with their handlers and consumers.
- **`shape_check`** — detect mismatches between response keys emitted by a handler
  and the property keys accessed by its consumers.
- **`api_impact`** — pre-change report for an API route handler: combines
  `route_map` + `shape_check` + `impact` data.

## Work
- `route_map`: query HANDLES_ROUTE + FETCHES edges to build a consumer list per
  route; include middleware chain info.
- `shape_check`: extract `responseKeys` from handler (best-effort: parse
  `.json({...})` return expressions); cross-reference with field accesses by
  consumers; report MISMATCH when consumer accesses missing keys.
- `api_impact`: orchestrate `route_map` + `shape_check` + run `impact()` on the
  handler symbol; accept `route` or `file` param; compute risk level.
- Register all three as MCP tool handlers in the server (issue #13).

**Depends on:** #9, #16, #13' \
  "feat" "p4-mcp-extended" "mcp" "routes"

# ── Issue 21 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: multi-repo indexing support and \`list_repos\` MCP tool" \
'## Summary
GitNexus can index multiple repositories and serve them from a single MCP
server instance. The `list_repos` tool lets agents discover which repos are
indexed before using name-scoped tools.

## Work
- Support a `--repos-dir` root that contains multiple named index subdirectories.
- Add a repo registry file (`repos.json`) listing indexed repos with name, path,
  indexed-at timestamp, and stats.
- The `serve` and `mcp` commands load all repos from the registry at startup.
- All existing MCP tools accept an optional `repo` parameter for routing.
- Implement the `list_repos` tool handler (returns name, path, indexed date,
  last commit, stats for each repo).

**Depends on:** #1, #13' \
  "feat" "p5-multi-repo" "mcp"

# ── Issue 22 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: multi-repo groups, contract registry, and group_* MCP tools" \
'## Summary
GitNexus supports grouping multiple repos into a "group" defined by a
`group.yaml` config file. HTTP contracts are extracted from each repo and
cross-linked across the group. Group-level MCP tools enable cross-repo search
and analysis.

## Work
- Define `group.yaml` schema: `name`, `repos` list with paths and roles.
- Add `rust-sniffer group init/add/remove` CLI subcommands.
- `group_sync`: build `contracts.json` by extracting Route nodes from each
  member repo and running exact-match → BM25 → embedding cascade cross-linking.
- `group_contracts`: inspect and filter `contracts.json`.
- `group_query`: run hybrid search across all group repos, merge via RRF.
- `group_status`: report index staleness (commit vs HEAD) per repo.
- `group_list`: list all configured groups with member details.
- Register all `group_*` tools in the MCP server (issue #13).

**Depends on:** #9, #12, #21, #13' \
  "feat" "p5-multi-repo" "mcp"

# ── Issue 23 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: \`wiki\` command — generate markdown wiki from the knowledge graph" \
'## Summary
GitNexus generates a structured markdown wiki from the knowledge graph, with
one page per community and one page per major execution flow. Pages include
symbol tables, call chain descriptions, and optional AI-generated summaries.

## Work
- Add `rust-sniffer wiki [--out <DIR>]` CLI subcommand.
- For each Community (issue #6): generate an index page listing member symbols,
  entry points, and an AI-generated description (opt-in; requires LLM config).
- For each Process (issue #7): generate a step-by-step trace page with symbol
  names, file paths, and call sequence.
- Cross-link pages via relative markdown links.
- Support `--no-llm` mode to output structural wiki without AI summaries.

**Depends on:** #6, #7' \
  "feat" "p6-ai"

# ── Issue 24 ──────────────────────────────────────────────────────────────────

new_issue \
  "feat: \`augment\` command — AI-powered enrichment of community and process labels" \
'## Summary
GitNexus has an `augment` command that calls an LLM to generate human-readable
heuristic labels and descriptions for Community and Process nodes
(e.g. "Auth" → "JWT-based authentication and session management").

## Work
- Add `rust-sniffer augment [OPTIONS]` CLI subcommand.
- For each unlabelled Community: pass member symbol names + top keywords to the
  LLM; receive `heuristicLabel` + `description`.
- For each unlabelled Process: pass entry point name + ordered step list; receive
  `heuristicLabel`.
- Persist enriched labels back to the graph DB (issue #1).
- Accept `--provider`, `--model` flags; read API key from environment variable.

**Depends on:** #6, #7' \
  "feat" "p6-ai"

# ── Issue 25 ──────────────────────────────────────────────────────────────────

new_issue \
  "perf: parallel parsing worker pool for large Rust codebases" \
'## Summary
GitNexus spawns worker threads to parse files in parallel, significantly reducing
indexing time for large repos. rust-sniffer currently parses files sequentially.

## Work
- Integrate `rayon` (or a Tokio task pool) for parallel file parsing.
- Dispatch each changed file to the pool; collect `Vec<FileSymbols>` results.
- Preserve deterministic ordering (sort by path after parallel collection).
- Benchmark on a large Rust repo (e.g. the Rust compiler sources) and report
  wall-clock improvement vs. sequential baseline.
- Add a `--no-parallel` flag as an escape hatch for debugging or constrained
  environments.

**Depends on:** none (pure performance improvement — can be applied at any phase)' \
  "perf" "p1-analysis"

echo ""
echo "Done — 25 issues created in $REPO"
