use crate::error::{GlossError, Result};
use crate::git::GitRepo;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DerivedState {
    #[serde(default)]
    pub edits: BTreeMap<String, String>,
    #[serde(default)]
    pub files: BTreeMap<String, FileState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileState {
    pub source_hash: String,
    pub header_updated: DateTime<Utc>,
}

impl DerivedState {
    pub fn load(repo: &GitRepo) -> Result<Self> {
        let path = index_path(repo);
        if !path.exists() {
            return Ok(Self::default());
        }
        let input = fs::read_to_string(&path).map_err(|error| GlossError::io(error, &path))?;
        serde_json::from_str(&input).map_err(|error| {
            GlossError::new(
                crate::error::ErrorCode::InvalidFormat,
                format!("invalid derived state: {error}"),
            )
            .file(path)
        })
    }

    pub fn save(&self, repo: &GitRepo) -> Result<()> {
        let path = index_path(repo);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| GlossError::io(error, parent))?;
        }
        let mut output = serde_json::to_string_pretty(self).expect("state is serializable");
        output.push('\n');
        fs::write(&path, output).map_err(|error| GlossError::io(error, path))
    }

    pub fn file(&self, relative: &Path) -> Option<&FileState> {
        self.files.get(&key(relative))
    }

    pub fn record_file(&mut self, relative: &Path, source: &[u8], updated: DateTime<Utc>) {
        let source_hash = hash(source);
        self.files.insert(
            key(relative),
            FileState {
                source_hash,
                header_updated: updated,
            },
        );
    }

    pub fn remove_file(&mut self, relative: &Path) {
        self.files.remove(&key(relative));
    }
}

pub fn hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn index_path(repo: &GitRepo) -> std::path::PathBuf {
    repo.git_dir().join("annotations").join("index.json")
}

fn key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
