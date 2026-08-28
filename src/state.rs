use crate::error::{GlossError, Result};
use crate::git::GitRepo;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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

    pub fn record_file(
        &mut self,
        repo: &GitRepo,
        relative: &Path,
        source: &[u8],
        updated: DateTime<Utc>,
    ) -> Result<()> {
        let source_hash = hash(source);
        let snapshot = snapshot_path(repo, &source_hash);
        if let Some(parent) = snapshot.parent() {
            fs::create_dir_all(parent).map_err(|error| GlossError::io(error, parent))?;
        }
        if !snapshot.exists() {
            fs::write(&snapshot, source).map_err(|error| GlossError::io(error, &snapshot))?;
        }
        self.files.insert(
            key(relative),
            FileState {
                source_hash,
                header_updated: updated,
            },
        );
        Ok(())
    }

    pub fn source_snapshot(&self, repo: &GitRepo, relative: &Path) -> Option<Vec<u8>> {
        let state = self.file(relative)?;
        fs::read(snapshot_path(repo, &state.source_hash)).ok()
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

fn snapshot_path(repo: &GitRepo, source_hash: &str) -> PathBuf {
    repo.git_dir()
        .join("annotations")
        .join("snapshots")
        .join(source_hash)
}

fn key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
