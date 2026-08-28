use serde::Serialize;
use std::path::Path;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, GlossError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidFormat,
    InvalidUuid,
    DuplicateEditId,
    MissingSource,
    MissingGloss,
    OrphanedGloss,
    InvalidRange,
    GlossOutsideHunk,
    StaleGloss,
    AmbiguousRepair,
    OutdatedHeader,
    GitError,
    IoError,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidFormat => "invalid_format",
            Self::InvalidUuid => "invalid_uuid",
            Self::DuplicateEditId => "duplicate_edit_id",
            Self::MissingSource => "missing_source",
            Self::MissingGloss => "missing_gloss",
            Self::OrphanedGloss => "orphaned_gloss",
            Self::InvalidRange => "invalid_range",
            Self::GlossOutsideHunk => "gloss_outside_hunk",
            Self::StaleGloss => "stale_gloss",
            Self::AmbiguousRepair => "ambiguous_repair",
            Self::OutdatedHeader => "outdated_header",
            Self::GitError => "git_error",
            Self::IoError => "io_error",
        }
    }
}

#[derive(Debug, Error, Clone, Serialize)]
#[error("{message}")]
pub struct GlossError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<[u32; 2]>,
}

impl GlossError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            file: None,
            edit_id: None,
            range: None,
        }
    }

    pub fn file(mut self, path: impl AsRef<Path>) -> Self {
        self.file = Some(path.as_ref().to_string_lossy().replace('\\', "/"));
        self
    }

    pub fn edit(mut self, id: impl ToString, start: u32, end: u32) -> Self {
        self.edit_id = Some(id.to_string());
        self.range = Some([start, end]);
        self
    }

    pub fn io(error: std::io::Error, path: impl AsRef<Path>) -> Self {
        Self::new(ErrorCode::IoError, error.to_string()).file(path)
    }
}
