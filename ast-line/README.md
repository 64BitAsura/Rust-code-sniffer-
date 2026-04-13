# rust-sniffer

A fast, tree-sitter–based Rust source-code indexer written in Rust, with **incremental re-indexing** support and an embedded **Symbol Explorer web UI**.

## Features

- **Symbol extraction** — functions (including `async`), structs, enums, traits, `impl` blocks (plain and trait-impl), modules, type aliases, constants, statics, macros, and struct fields.
- **Rich metadata** — visibility (`public` / `restricted` / `private`), line ranges, return types, field types, and `is_async` flag.
- **Incremental indexing** — SHA-256 fingerprints (16-char prefix) are persisted in `.rust-sniffer/hashes.json`; on subsequent runs only changed files are re-parsed.
- **JSON output** — every indexed file emits structured JSON suitable for further tooling.
- **`status`** — inspect the index at a glance: file count, symbol count, and when it was last built.
- **`clean`** — safely delete the index directory with a `--force` guard.
- **`serve`** — start a local HTTP server that exposes a REST API and a built-in Symbol Explorer web UI (no separate install needed).

## Installation

```bash
cd rust-sniffer
cargo build --release
# binary is at target/release/rust-sniffer
```

## Usage

### Full index

```bash
rust-sniffer index [ROOT] [OPTIONS]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--index-dir <DIR>` | `.rust-sniffer` | Where hash state and symbol cache are stored |
| `--incremental` / `-i` | off | Only re-parse changed files |
| `--verbose` / `-v` | off | Print per-file progress to stderr |
| `--pretty` / `-p` | off | Pretty-print the JSON output |

**First run (full parse):**

```bash
rust-sniffer index ./my-project --incremental --verbose --pretty
```

```
  parsing  src/lib.rs
  parsing  src/main.rs
Indexed 2 file(s): 2 parsed, 0 cached, 0 removed, 15 symbols total
[
  {
    "path": "src/lib.rs",
    "hash": "4e934c9d8c94b7b5",
    "symbols": [ ... ]
  }
]
```

**Second run (incremental — nothing changed):**

```bash
rust-sniffer index ./my-project --incremental --verbose
```

```
  cached   src/lib.rs
  cached   src/main.rs
Indexed 2 file(s): 0 parsed, 2 cached, 0 removed, 15 symbols total
```

### Diff — preview what would be re-parsed

```bash
rust-sniffer diff [ROOT] [OPTIONS]
```

```
No changes detected — all 2 file(s) are up to date.
```

Or, if a file was modified:

```
1 file(s) changed, 1 file(s) unchanged:
  M  src/lib.rs
```

### Status — inspect the current index

```bash
rust-sniffer status [--index-dir <DIR>]
```

```
Index directory:  .rust-sniffer
Root:             /home/user/my-project
Indexed at:       2024-03-15T10:30:00+00:00
Files indexed:    42
Total symbols:    1,234
```

If no index exists yet:

```
No index found at '.rust-sniffer'.
Run:  rust-sniffer index --incremental
```

### Clean — delete the index

```bash
rust-sniffer clean [--index-dir <DIR>] [--force]
```

Without `--force`, shows what would be deleted:

```
This will delete the index at '.rust-sniffer'.
Run with --force to confirm deletion.
```

With `--force`:

```
Deleted '.rust-sniffer'.
```

### Serve — Symbol Explorer web UI + REST API

```bash
rust-sniffer serve [OPTIONS]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--index-dir <DIR>` | `.rust-sniffer` | Where the index is stored |
| `--port <PORT>` | `3741` | TCP port to listen on |
| `--host <HOST>` | `localhost` | Bind address |

```
rust-sniffer serve  listening on  http://localhost:3741
  GET /            — Symbol Explorer web UI
  GET /api/status  — index metadata (JSON)
  GET /api/symbols — symbol list (JSON)
```

Open `http://localhost:3741` in a browser to browse all indexed symbols, filter by kind, and inspect per-symbol metadata (return type, field type, line range, `async` flag, etc.).

**Tip:** run `index --incremental` first, then `serve` to explore the results interactively.

```bash
rust-sniffer index . --incremental
rust-sniffer serve
```

#### REST API

Both endpoints return JSON and support cross-origin requests (CORS `*`).

| Endpoint | Description |
|----------|-------------|
| `GET /api/status` | Index metadata: `indexed_at`, `root`, `file_count`, `symbol_count` |
| `GET /api/symbols` | Full array of `FileSymbols` objects (same schema as `index` JSON output) |

## Output format

Each element in the top-level JSON array corresponds to one file:

```json
{
  "path": "src/lib.rs",
  "hash": "4e934c9d8c94b7b5",
  "symbols": [
    {
      "name": "MyStruct",
      "kind": "struct",
      "visibility": "public",
      "start_line": 5,
      "end_line": 12,
      "trait_name": null,
      "field_type": null,
      "return_type": null,
      "is_async": false
    },
    {
      "name": "fetch",
      "kind": "function",
      "visibility": "public",
      "start_line": 15,
      "end_line": 22,
      "trait_name": null,
      "field_type": null,
      "return_type": "Result<Vec<u8>, Error>",
      "is_async": true
    }
  ]
}
```

### Symbol kinds

| `kind` | Description |
|--------|-------------|
| `function` | Free function or method |
| `struct` | Struct definition |
| `enum` | Enum definition |
| `trait` | Trait definition |
| `impl` | Concrete `impl` block |
| `trait_impl` | `impl Trait for Type` block |
| `module` | `mod` declaration |
| `type_alias` | `type Foo = ...` |
| `constant` | `const` item |
| `static` | `static` item |
| `macro` | `macro_rules!` definition |
| `field` | Named struct field |

## Incremental indexing internals

On each `index --incremental` run:

1. All `*.rs` files under `ROOT` are discovered and their SHA-256 prefix fingerprints computed.
2. Fingerprints are compared to `.rust-sniffer/hashes.json` (loaded from the previous run).
3. **Changed / new** files are re-parsed by tree-sitter.
4. **Unchanged** files have their symbols loaded from `.rust-sniffer/symbols.json`.
5. Deleted files are removed from the state.
6. The updated state and symbol cache are written back to `.rust-sniffer/`.

This mirrors the strategy used by the GitNexus TypeScript pipeline
(`RepoMeta.fileHashes` + `diffFileHashes()`), re-implemented natively in Rust.

After each successful `index` run a `meta.json` file is also written to the
index directory — it records the root path, indexed-at timestamp, file count,
and symbol count. The `status` and `serve` commands read this file to avoid
having to re-scan the project just to report statistics.

## Development

```bash
cd rust-sniffer
cargo test          # unit + integration tests (21 tests)
cargo build         # debug build
cargo build --release  # optimised binary
```
