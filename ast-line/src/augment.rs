//! `augment` command — AI-powered enrichment of Community and Process labels.
//!
//! Calls an OpenAI-compatible LLM to generate human-readable `heuristicLabel`
//! and `description` values for Community nodes and `heuristicLabel` values
//! for Process nodes that currently have only auto-generated labels.
//!
//! ## Environment variables
//!
//! | Variable | Purpose |
//! |----------|---------|
//! | `AUGMENT_API_KEY` | Bearer token for the LLM API (required) |
//! | `AUGMENT_API_URL` | Base URL (default: `https://api.openai.com`) |
//!
//! ## Example
//!
//! ```text
//! ast-line augment --provider openai --model gpt-4o-mini
//! ```

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::community::{load_communities, save_communities, Community};
use crate::error::SnifferError;
use crate::process::{load_processes, save_processes, Process};

// ─── Config ───────────────────────────────────────────────────────────────────

/// Configuration for an augment run.
#[derive(Debug, Clone)]
pub struct AugmentConfig {
    /// Provider name (informational only; controls default base URL).
    pub provider: String,
    /// Model identifier, e.g. `"gpt-4o-mini"`.
    pub model: String,
    /// LLM API base URL (without trailing slash).
    pub api_url: String,
    /// Bearer token for the API.
    pub api_key: String,
    /// Maximum number of API calls to make (0 = unlimited).
    pub max_calls: usize,
    /// When `true`, print progress to stderr.
    pub verbose: bool,
}

impl AugmentConfig {
    /// Build from flags + environment variables.
    ///
    /// Reads `AUGMENT_API_KEY` and (optionally) `AUGMENT_API_URL` from env.
    pub fn from_env(provider: &str, model: &str, verbose: bool) -> Result<Self, SnifferError> {
        let api_key = std::env::var("AUGMENT_API_KEY").map_err(|_| {
            SnifferError::Io("AUGMENT_API_KEY environment variable is not set".to_owned())
        })?;

        let api_url = std::env::var("AUGMENT_API_URL").unwrap_or_else(|_| {
            if provider.to_lowercase().contains("openai") {
                "https://api.openai.com".to_owned()
            } else {
                "https://api.openai.com".to_owned()
            }
        });

        Ok(Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
            api_url,
            api_key,
            max_calls: 0,
            verbose,
        })
    }
}

// ─── OpenAI-compatible request / response ─────────────────────────────────────

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessageContent,
}

#[derive(Deserialize)]
struct ChatMessageContent {
    content: String,
}

// ─── LLM call helper ──────────────────────────────────────────────────────────

/// Call the chat-completions endpoint and return the assistant reply.
fn chat_complete(config: &AugmentConfig, prompt: &str) -> Result<String, SnifferError> {
    let url = format!("{}/v1/chat/completions", config.api_url.trim_end_matches('/'));

    let body = ChatRequest {
        model: config.model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_owned(),
                content: "You are a helpful code-analysis assistant. \
                          Reply with only the requested JSON, no markdown fences."
                    .to_owned(),
            },
            ChatMessage {
                role: "user".to_owned(),
                content: prompt.to_owned(),
            },
        ],
        temperature: 0.2,
        max_tokens: 128,
    };

    // Use a blocking reqwest client (tokio runtime already present at binary level,
    // but augment is invoked from a sync context, so we build a one-shot runtime).
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| SnifferError::Io(format!("building tokio runtime: {e}")))?;

    let response_text = rt.block_on(async {
        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .bearer_auth(&config.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| SnifferError::Io(format!("LLM request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(SnifferError::Io(format!(
                "LLM API returned {status}: {text}"
            )));
        }

        resp.text()
            .await
            .map_err(|e| SnifferError::Io(format!("reading LLM response: {e}")))
    })?;

    let parsed: ChatResponse = serde_json::from_str(&response_text)
        .map_err(|e| SnifferError::Json(format!("parsing LLM response: {e}")))?;

    Ok(parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .unwrap_or_default()
        .trim()
        .to_owned())
}

// ─── Label structs returned by the LLM ────────────────────────────────────────

#[derive(Deserialize)]
struct CommunityLabel {
    #[serde(rename = "heuristicLabel")]
    heuristic_label: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
struct ProcessLabel {
    #[serde(rename = "heuristicLabel")]
    heuristic_label: String,
}

// ─── Augmentation logic ────────────────────────────────────────────────────────

/// Augment community labels and descriptions using the LLM.
fn augment_community(config: &AugmentConfig, community: &mut Community) -> bool {
    // Only augment communities whose label looks auto-generated.
    let looks_auto = community.heuristic_label.starts_with("cluster_")
        || community.heuristic_label.is_empty()
        || community.keywords.iter().all(|k| k.len() <= 3);

    if !looks_auto {
        return false;
    }

    let symbol_names: Vec<&str> = community
        .symbol_ids
        .iter()
        .filter_map(|id| id.split("::").last())
        .take(20)
        .collect();

    let prompt = format!(
        "Given these Rust symbol names from a single functional area: {symbols}\n\
         Top keywords: {keywords}\n\n\
         Return JSON: {{\"heuristicLabel\": \"<short label>\", \"description\": \"<one sentence>\"}}\n\
         The label should be 1-3 words, title-cased (e.g. \"Auth\", \"Data Access\").",
        symbols = symbol_names.join(", "),
        keywords = community.keywords.join(", "),
    );

    match chat_complete(config, &prompt) {
        Ok(reply) => {
            if let Ok(label) = serde_json::from_str::<CommunityLabel>(&reply) {
                community.heuristic_label = label.heuristic_label;
                // Store description in keywords[0] slot if present (backwards-compat).
                if !label.description.is_empty() {
                    community.keywords.insert(0, label.description);
                    community.keywords.truncate(4);
                }
                true
            } else {
                false
            }
        }
        Err(e) => {
            eprintln!("  warning: LLM call failed for community {}: {e}", community.uid);
            false
        }
    }
}

/// Augment process heuristic labels using the LLM.
fn augment_process(config: &AugmentConfig, process: &mut Process) -> bool {
    // Only augment processes whose label looks auto-generated or is a bare fn name.
    let looks_auto = process.heuristic_label == process.name
        || process.heuristic_label.starts_with("process_");

    if !looks_auto {
        return false;
    }

    let step_names: Vec<&str> = process
        .steps
        .iter()
        .filter_map(|id| id.split("::").last())
        .take(10)
        .collect();

    let prompt = format!(
        "Entry point: `{entry}`\n\
         Call steps: {steps}\n\n\
         Return JSON: {{\"heuristicLabel\": \"<short label>\"}}\n\
         The label should be 2-4 words describing what this execution flow does \
         (e.g. \"Handle Login Request\").",
        entry = process.name,
        steps = step_names.join(" → "),
    );

    match chat_complete(config, &prompt) {
        Ok(reply) => {
            if let Ok(label) = serde_json::from_str::<ProcessLabel>(&reply) {
                process.heuristic_label = label.heuristic_label;
                true
            } else {
                false
            }
        }
        Err(e) => {
            eprintln!("  warning: LLM call failed for process {}: {e}", process.uid);
            false
        }
    }
}

// ─── Public entry point ────────────────────────────────────────────────────────

/// Run the augment pipeline on the index at `index_dir`.
///
/// Loads communities and processes, calls the LLM to enrich labels, and
/// persists the updated data back to disk.
pub fn run_augment(index_dir: &Path, config: &AugmentConfig) -> Result<AugmentSummary, SnifferError> {
    let mut communities = load_communities(index_dir);
    let mut processes = load_processes(index_dir);

    if communities.is_empty() && processes.is_empty() {
        return Err(SnifferError::Io(
            "No communities or processes found. Run `ast-line index` first.".to_owned(),
        ));
    }

    let mut community_enriched = 0usize;
    let mut process_enriched = 0usize;
    let mut calls = 0usize;

    // Augment communities
    for community in &mut communities {
        if config.max_calls > 0 && calls >= config.max_calls {
            break;
        }
        if config.verbose {
            eprintln!("  augmenting community {} …", community.uid);
        }
        if augment_community(config, community) {
            community_enriched += 1;
            calls += 1;
        }
    }

    // Augment processes
    for process in &mut processes {
        if config.max_calls > 0 && calls >= config.max_calls {
            break;
        }
        if config.verbose {
            eprintln!("  augmenting process {} …", process.uid);
        }
        if augment_process(config, process) {
            process_enriched += 1;
            calls += 1;
        }
    }

    // Persist enriched data
    save_communities(index_dir, &communities)?;
    save_processes(index_dir, &processes)?;

    Ok(AugmentSummary {
        community_enriched,
        process_enriched,
        total_llm_calls: calls,
    })
}

/// Summary of an augment run.
#[derive(Debug)]
pub struct AugmentSummary {
    /// Number of communities whose label was updated.
    pub community_enriched: usize,
    /// Number of processes whose label was updated.
    pub process_enriched: usize,
    /// Total number of LLM API calls made.
    pub total_llm_calls: usize,
}
