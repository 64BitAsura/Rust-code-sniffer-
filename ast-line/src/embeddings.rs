//! Optional vector embeddings via an HTTP embedding provider.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::SnifferError;

const EMBEDDINGS_FILE: &str = "embeddings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    pub api_url: String,
    pub api_key: String,
    pub model: String,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        EmbeddingConfig {
            api_url: "https://api.openai.com/v1/embeddings".to_owned(),
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            model: "text-embedding-3-small".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmbeddingStore {
    pub embeddings: HashMap<String, Vec<f32>>,
}

impl EmbeddingStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: String, embedding: Vec<f32>) {
        self.embeddings.insert(id, embedding);
    }

    pub fn get(&self, id: &str) -> Option<&Vec<f32>> {
        self.embeddings.get(id)
    }

    pub fn len(&self) -> usize {
        self.embeddings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.embeddings.is_empty()
    }
}

pub async fn generate_embeddings(
    texts: &[(String, String)],
    config: &EmbeddingConfig,
) -> Result<EmbeddingStore, SnifferError> {
    let mut store = EmbeddingStore::new();
    if config.api_key.is_empty() {
        return Ok(store);
    }

    let client = reqwest::Client::new();
    for (id, text) in texts {
        let body = serde_json::json!({ "model": config.model, "input": text });
        let resp = client
            .post(&config.api_url)
            .bearer_auth(&config.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| SnifferError::Io(format!("embedding request failed: {e}")))?;
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| SnifferError::Json(format!("embedding response parse: {e}")))?;
        if let Some(emb) = json["data"][0]["embedding"].as_array() {
            let vec: Vec<f32> =
                emb.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect();
            store.insert(id.clone(), vec);
        }
    }
    Ok(store)
}

pub fn save_embeddings(index_dir: &Path, store: &EmbeddingStore) -> Result<(), SnifferError> {
    fs::create_dir_all(index_dir)
        .map_err(|e| SnifferError::Io(format!("creating index dir: {e}")))?;
    let json = serde_json::to_string_pretty(store)
        .map_err(|e| SnifferError::Json(format!("serialising embeddings: {e}")))?;
    fs::write(index_dir.join(EMBEDDINGS_FILE), json)
        .map_err(|e| SnifferError::Io(format!("writing embeddings.json: {e}")))?;
    Ok(())
}

pub fn load_embeddings(index_dir: &Path) -> EmbeddingStore {
    let path = index_dir.join(EMBEDDINGS_FILE);
    let raw = match fs::read_to_string(path) {
        Ok(r) => r,
        Err(_) => return EmbeddingStore::new(),
    };
    serde_json::from_str(&raw).unwrap_or_default()
}
