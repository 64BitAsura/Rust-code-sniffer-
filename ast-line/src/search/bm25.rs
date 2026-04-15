//! BM25 full-text search index over symbol names and file paths.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

const K1: f64 = 1.2;
const B: f64 = 0.75;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub symbol_id: String,
    pub score: f64,
    pub name: String,
    pub file_path: String,
}

pub struct BM25Index {
    inverted: HashMap<String, HashMap<String, usize>>,
    doc_lengths: HashMap<String, usize>,
    doc_meta: HashMap<String, (String, String)>,
    avg_doc_len: f64,
}

impl BM25Index {
    pub fn new() -> Self {
        BM25Index {
            inverted: HashMap::new(),
            doc_lengths: HashMap::new(),
            doc_meta: HashMap::new(),
            avg_doc_len: 0.0,
        }
    }

    pub fn add_document(&mut self, doc_id: String, name: &str, file_path: &str) {
        let tokens = tokenize(&format!("{name} {file_path}"));
        let doc_len = tokens.len();
        self.doc_lengths.insert(doc_id.clone(), doc_len);
        self.doc_meta.insert(doc_id.clone(), (name.to_owned(), file_path.to_owned()));
        for token in tokens {
            *self.inverted.entry(token).or_default().entry(doc_id.clone()).or_insert(0) += 1;
        }
    }

    pub fn build(&mut self) {
        let total: usize = self.doc_lengths.values().sum();
        self.avg_doc_len = if self.doc_lengths.is_empty() {
            1.0
        } else {
            total as f64 / self.doc_lengths.len() as f64
        };
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let n = self.doc_lengths.len() as f64;
        let query_terms = tokenize(query);
        let mut scores: HashMap<String, f64> = HashMap::new();

        for term in &query_terms {
            let docs = match self.inverted.get(term) {
                Some(d) => d,
                None => continue,
            };
            let df = docs.len() as f64;
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
            for (doc_id, &tf) in docs {
                let dl = *self.doc_lengths.get(doc_id).unwrap_or(&1) as f64;
                let tf_norm = (tf as f64 * (K1 + 1.0))
                    / (tf as f64 + K1 * (1.0 - B + B * dl / self.avg_doc_len));
                *scores.entry(doc_id.clone()).or_insert(0.0) += idf * tf_norm;
            }
        }

        let mut results: Vec<SearchResult> = scores
            .into_iter()
            .filter_map(|(doc_id, score)| {
                let (name, file_path) = self.doc_meta.get(&doc_id)?.clone();
                Some(SearchResult { symbol_id: doc_id, score, name, file_path })
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        results
    }

    pub fn doc_count(&self) -> usize {
        self.doc_lengths.len()
    }
}

impl Default for BM25Index {
    fn default() -> Self {
        Self::new()
    }
}

fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 1)
        .map(|t| t.to_lowercase())
        .collect()
}
