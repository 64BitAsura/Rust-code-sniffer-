use std::collections::HashMap; use std::fs; use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::error::SnifferError;
const GROUP_CONFIG_FILE: &str = "groups.json";
#[derive(Debug, Clone, Serialize, Deserialize)] pub struct GroupRepo { pub name: String, pub index_dir: String }
#[derive(Debug, Clone, Serialize, Deserialize)] pub struct RepoGroup { pub name: String, pub description: Option<String>, pub repos: Vec<GroupRepo>, pub contracts: Vec<String> }
#[derive(Debug, Clone, Serialize, Deserialize, Default)] pub struct GroupConfig { pub groups: HashMap<String, RepoGroup> }
impl GroupConfig {
    pub fn new() -> Self { Self::default() }
    pub fn add_group(&mut self, g: RepoGroup) { self.groups.insert(g.name.clone(), g); }
    pub fn save(&self, d: &Path) -> Result<(), SnifferError> { fs::create_dir_all(d).map_err(|e| SnifferError::Io(format!("{e}")))?; let j = serde_json::to_string_pretty(self).map_err(|e| SnifferError::Json(format!("{e}")))?; fs::write(d.join(GROUP_CONFIG_FILE), j).map_err(|e| SnifferError::Io(format!("{e}")))?; Ok(()) }
    pub fn load(d: &Path) -> Self { let p = d.join(GROUP_CONFIG_FILE); let r = match fs::read_to_string(p) { Ok(r) => r, Err(_) => return Self::default() }; serde_json::from_str(&r).unwrap_or_default() }
}
