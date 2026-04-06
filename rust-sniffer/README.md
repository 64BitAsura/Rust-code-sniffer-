# rust-sniffer

A fast, tree-sitter–based Rust source-code indexer written in Rust, with **incremental re-indexing** support.

## Features

- **Symbol extraction** — functions (including `async`), structs, enums, traits, `impl` blocks (plain and trait-impl), modules, type aliases, constants, statics, macros, and struct fields.
- **Rich metadata** — visibility (`public` / `restricted` / `private`), line ranges, return types, field types, and `is_async` flag.
- **Incremental indexing** — SHA-256 fingerprints (16-char prefix) are persisted in `.rust-sniffer/hashes.json`; on subsequent runs only changed files are re-parsed.
- **JSON output** — every indexed file emits structured JSON suitable for further tooling.

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

## Development

```bash
cd rust-sniffer
cargo test          # unit + integration tests (21 tests)
cargo build         # debug build
cargo build --release  # optimised binary
```
