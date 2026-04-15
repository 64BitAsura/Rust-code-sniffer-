use std::collections::HashMap; use std::fs; use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::error::SnifferError;
const REGISTRY_FILE: &str = "registry.json";
#[derive(Debug, Clone, Serialize, Deserialize)] pub struct RepoEntry { pub name: String, pub root: String, pub index_dir: String, pub description: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize, Default)] pub struct RepoRegistry { pub repos: HashMap<String, RepoEntry> }
impl RepoRegistry {
    pub fn new() -> Self { Self::default() }
    pub fn register(&mut self, e: RepoEntry) { self.repos.insert(e.name.clone(), e); }
    pub fn get(&self, n: &str) -> Option<&RepoEntry> { self.repos.get(n) }
    pub fn list(&self) -> Vec<&RepoEntry> { self.repos.values().collect() }
    pub fn save(&self, d: &Path) -> Result<(), SnifferError> { fs::create_dir_all(d).map_err(|e| SnifferError::Io(format!("{e}")))?; let j = serde_json::to_string_pretty(self).map_err(|e| SnifferError::Json(format!("{e}")))?; fs::write(d.join(REGISTRY_FILE), j).map_err(|e| SnifferError::Io(format!("{e}")))?; Ok(()) }
    pub fn load(d: &Path) -> Self { let p = d.join(REGISTRY_FILE); let r = match fs::read_to_string(p) { Ok(r) => r, Err(_) => return Self::default() }; serde_json::from_str(&r).unwrap_or_default() }
}
