//! `rust-sniffer` CLI — Rust code indexer with incremental re-indexing.
//!
//! ## Commands
//!
//! ```text
//! rust-sniffer index [OPTIONS] [ROOT]
//! rust-sniffer diff  [OPTIONS] [ROOT]
//! ```

use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

use rust_sniffer::indexer::{run_index, IndexOptions};

/// Rust source-code indexer with incremental re-indexing support.
#[derive(Parser)]
#[command(name = "rust-sniffer", version, about, long_about = None)]
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
        #[arg(short, long, default_value = ".rust-sniffer")]
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
    },

    /// Show which files would be re-parsed if `index --incremental` were run.
    Diff {
        /// Root directory to scan (defaults to current directory).
        #[arg(default_value = ".")]
        root: PathBuf,

        /// Directory where the index state is stored.
        #[arg(short, long, default_value = ".rust-sniffer")]
        index_dir: PathBuf,
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
        } => {
            let opts = IndexOptions {
                root: root.clone(),
                index_dir: index_dir.clone(),
                incremental: *incremental,
                verbose: *verbose,
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
            use rust_sniffer::incremental::{diff_files, HashState};
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
    }
}
