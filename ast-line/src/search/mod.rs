pub mod bm25;
pub use bm25::{BM25Index, SearchResult};

/// Reciprocal Rank Fusion constant (typically 60).
///
/// Higher values reduce the score differences between ranks.
const RRF_K: f64 = 60.0;

/// Merge two ranked lists using Reciprocal Rank Fusion.
pub fn rrf_merge(bm25_results: &[SearchResult], vector_results: &[SearchResult]) -> Vec<SearchResult> {
    use std::collections::HashMap;

    let mut scores: HashMap<String, f64> = HashMap::new();
    let mut meta: HashMap<String, (String, String)> = HashMap::new();

    for (rank, r) in bm25_results.iter().enumerate() {
        *scores.entry(r.symbol_id.clone()).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
        meta.entry(r.symbol_id.clone()).or_insert_with(|| (r.name.clone(), r.file_path.clone()));
    }
    for (rank, r) in vector_results.iter().enumerate() {
        *scores.entry(r.symbol_id.clone()).or_default() += 1.0 / (RRF_K + rank as f64 + 1.0);
        meta.entry(r.symbol_id.clone()).or_insert_with(|| (r.name.clone(), r.file_path.clone()));
    }

    let mut merged: Vec<SearchResult> = scores.into_iter().map(|(id, score)| {
        let (name, file_path) = meta.get(&id).cloned().unwrap_or_default();
        SearchResult { symbol_id: id, score, name, file_path }
    }).collect();

    merged.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    merged
}

/// Run hybrid search: BM25 + optional vector, merged via RRF.
pub fn hybrid_search(
    index: &BM25Index,
    query: &str,
    vector_results: &[SearchResult],
    limit: usize,
) -> Vec<SearchResult> {
    let bm25_results = index.search(query, limit * 2);
    let mut merged = rrf_merge(&bm25_results, vector_results);
    merged.truncate(limit);
    merged
}
