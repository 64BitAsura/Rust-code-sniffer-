//! `ast-line` CLI — Rust code indexer with incremental re-indexing.
//!
//! ## Commands
//!
//! ```text
//! ast-line index  [OPTIONS] [ROOT]   — Index a Rust project
//! ast-line diff   [OPTIONS] [ROOT]   — Preview incremental changes
//! ast-line status [OPTIONS]          — Show index status
//! ast-line clean  [OPTIONS]          — Delete the index
//! ast-line serve  [OPTIONS]          — Start the web UI + REST API server
//! ```

use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

use ast_line::indexer::{run_index, IndexOptions};

/// Rust source-code indexer with incremental re-indexing support.
#[derive(Parser)]
#[command(name = "ast-line", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Index a Rust project and output symbols as JSON.
    Index {
        /// Root directory to scan (defaults to current directory).
        #[arg(default_value = ".")]
        root: PathBuf,

        /// Directory where the index state is stored.
        #[arg(long, default_value = ".ast-line")]
        index_dir: PathBuf,

        /// Only re-parse files whose content has changed since the last run.
        #[arg(short, long, default_value_t = false)]
        incremental: bool,

        /// Print progress to stderr.
        #[arg(short, long, default_value_t = false)]
        verbose: bool,

        /// Pretty-print the JSON output.
        #[arg(short, long, default_value_t = false)]
        pretty: bool,

        /// Generate vector embeddings for symbols.
        #[arg(long, default_value_t = false)]
        embeddings: bool,
    },

    /// Show which files would be re-parsed if `index --incremental` were run.
    Diff {
        /// Root directory to scan (defaults to current directory).
        #[arg(default_value = ".")]
        root: PathBuf,

        /// Directory where the index state is stored.
        #[arg(long, default_value = ".ast-line")]
        index_dir: PathBuf,
    },

    /// Show the current index status (file count, symbol count, last indexed time).
    Status {
        /// Directory where the index state is stored.
        #[arg(long, default_value = ".ast-line")]
        index_dir: PathBuf,
    },

    /// Delete the index directory.
    Clean {
        /// Directory where the index state is stored.
        #[arg(long, default_value = ".ast-line")]
        index_dir: PathBuf,

        /// Skip the confirmation prompt and delete immediately.
        #[arg(short, long, default_value_t = false)]
        force: bool,
    },

    /// Start the Symbol Explorer web UI and REST API server.
    Serve {
        /// Directory where the index state is stored.
        #[arg(long, default_value = ".ast-line")]
        index_dir: PathBuf,

        /// Port to listen on.
        #[arg(short, long, default_value_t = 3741)]
        port: u16,

        /// Host address to bind.
        #[arg(long, default_value = "localhost")]
        host: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Index {
            root,
            index_dir,
            incremental,
            verbose,
            pretty,
            embeddings,
        } => {
            let opts = IndexOptions {
                root: root.clone(),
                index_dir: index_dir.clone(),
                incremental: *incremental,
                verbose: *verbose,
                generate_embeddings: *embeddings,
            };

            match run_index(&opts) {
                Ok((symbols, summary)) => {
                    if *verbose {
                        eprintln!(
                            "Indexed {} file(s): {} parsed, {} cached, {} removed, {} symbols total",
                            summary.total_files,
                            summary.parsed_files,
                            summary.skipped_files,
                            summary.removed_files,
                            summary.total_symbols,
                        );
                        eprintln!(
                            "Graph: {} node(s), {} edge(s)",
                            summary.graph_nodes,
                            summary.graph_edges,
                        );
                    }

                    let json = if *pretty {
                        serde_json::to_string_pretty(&symbols)
                    } else {
                        serde_json::to_string(&symbols)
                    };

                    match json {
                        Ok(output) => println!("{output}"),
                        Err(e) => {
                            eprintln!("error: failed to serialise output — {e}");
                            process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
            }
        }

        Commands::Diff { root, index_dir } => {
            use ast_line::incremental::{diff_files, HashState};
            use walkdir::WalkDir;

            // Collect *.rs files
            let mut files: Vec<(String, Vec<u8>)> = Vec::new();
            for entry in WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path().to_path_buf();
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                if let Ok(content) = std::fs::read(&path) {
                    let key = path
                        .strip_prefix(root)
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| path.to_string_lossy().into_owned());
                    files.push((key, content));
                }
            }
            files.sort_by(|a, b| a.0.cmp(&b.0));

            let state = match HashState::load(index_dir) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("warning: could not load hash state — {e}");
                    HashState::default()
                }
            };

            let diff = diff_files(&files, &state);

            if diff.changed.is_empty() {
                println!("No changes detected — all {} file(s) are up to date.", files.len());
            } else {
                println!(
                    "{} file(s) changed, {} file(s) unchanged:",
                    diff.changed.len(),
                    diff.unchanged.len()
                );
                for path in &diff.changed {
                    println!("  M  {}", path.display());
                }
            }
        }

        Commands::Status { index_dir } => {
            use ast_line::meta::IndexMeta;

            match IndexMeta::load(index_dir) {
                Some(meta) => {
                    println!("Index directory:  {}", index_dir.display());
                    println!("Root:             {}", meta.root);
                    println!("Indexed at:       {}", meta.indexed_at);
                    println!("Files indexed:    {}", meta.file_count);
                    println!("Total symbols:    {}", meta.symbol_count);
                    println!("Graph nodes:      {}", meta.graph_node_count);
                    println!("Graph edges:      {}", meta.graph_edge_count);
                }
                None => {
                    println!("No index found at '{}'.", index_dir.display());
                    println!("Run:  ast-line index --incremental");
                }
            }
        }

        Commands::Clean { index_dir, force } => {
            if !index_dir.exists() {
                println!("No index found at '{}'.", index_dir.display());
                return;
            }

            if !force {
                println!(
                    "This will delete the index at '{}'.",
                    index_dir.display()
                );
                println!("Run with --force to confirm deletion.");
                return;
            }

            match std::fs::remove_dir_all(index_dir) {
                Ok(()) => println!("Deleted '{}'.", index_dir.display()),
                Err(e) => {
                    eprintln!("error: failed to delete '{}' — {e}", index_dir.display());
                    process::exit(1);
                }
            }
        }

        Commands::Serve {
            index_dir,
            port,
            host,
        } => {
            use ast_line::server::run_server;

            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to build Tokio runtime");

            if let Err(e) = rt.block_on(run_server(index_dir, host, *port)) {
                eprintln!("error: {e}");
                process::exit(1);
            }
        }
    }
}
