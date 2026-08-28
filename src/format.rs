use crate::error::{ErrorCode, GlossError, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

impl LineRange {
    pub fn new(start: u32, end: u32) -> Result<Self> {
        if start == 0 || end < start {
            return Err(GlossError::new(
                ErrorCode::InvalidRange,
                format!("invalid line range {start}:{end}"),
            )
            .edit("", start, end));
        }
        Ok(Self { start, end })
    }

    pub fn parse(value: &str) -> Result<Self> {
        let (start, end) = value.split_once(':').ok_or_else(|| {
            GlossError::new(
                ErrorCode::InvalidRange,
                format!("invalid line range {value}"),
            )
        })?;
        let start = start.parse().map_err(|_| {
            GlossError::new(
                ErrorCode::InvalidRange,
                format!("invalid line range {value}"),
            )
        })?;
        let end = end.parse().map_err(|_| {
            GlossError::new(
                ErrorCode::InvalidRange,
                format!("invalid line range {value}"),
            )
        })?;
        Self::new(start, end)
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

impl Display for LineRange {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.start, self.end)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GlossRecord {
    pub edit_id: Uuid,
    pub range: LineRange,
    pub timestamp: DateTime<Utc>,
    pub user: String,
    pub agent: String,
    pub session_id: String,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GlossFile {
    pub version: u8,
    pub updated: DateTime<Utc>,
    pub editor: String,
    pub records: Vec<GlossRecord>,
}

impl GlossFile {
    pub fn empty(updated: DateTime<Utc>, editor: impl Into<String>) -> Self {
        Self {
            version: VERSION,
            updated,
            editor: editor.into(),
            records: Vec::new(),
        }
    }

    pub fn parse(input: &str, path: &Path) -> Result<Self> {
        let mut lines = input.lines();
        let version_line = lines
            .next()
            .ok_or_else(|| invalid(path, "missing version header"))?;
        let version = version_line
            .strip_prefix("version: ")
            .ok_or_else(|| invalid(path, "expected `version: <number>`"))?
            .parse::<u8>()
            .map_err(|_| invalid(path, "version must be a number"))?;
        if version != VERSION {
            return Err(invalid(
                path,
                format!("unsupported gloss version {version}"),
            ));
        }

        let updated_line = lines
            .next()
            .ok_or_else(|| invalid(path, "missing updated header"))?;
        let updated = updated_line
            .strip_prefix("updated: ")
            .ok_or_else(|| invalid(path, "expected `updated: <timestamp>`"))?
            .parse::<DateTime<Utc>>()
            .map_err(|_| invalid(path, "updated must be an RFC 3339 timestamp"))?;

        let editor_line = lines
            .next()
            .ok_or_else(|| invalid(path, "missing editor header"))?;
        let editor = editor_line
            .strip_prefix("editor: ")
            .filter(|value| valid_token(value))
            .ok_or_else(|| invalid(path, "editor must be one non-empty token"))?
            .to_owned();

        if lines.next() != Some("") {
            return Err(invalid(path, "header must be followed by a blank line"));
        }

        let mut records = Vec::new();
        for (offset, line) in lines.enumerate() {
            if line.trim().is_empty() {
                return Err(invalid(
                    path,
                    format!("unexpected blank line at {}", offset + 5),
                ));
            }
            records.push(parse_record(line, path, offset + 5)?);
        }
        Ok(Self {
            version,
            updated,
            editor,
            records,
        })
    }

    pub fn render(&self) -> String {
        let mut output = format!(
            "version: {}\nupdated: {}\neditor: {}\n\n",
            self.version,
            timestamp(self.updated),
            self.editor
        );
        for record in &self.records {
            output.push_str(&format!(
                "{} {} {} {} {} {} {}\n",
                record.edit_id,
                record.range,
                timestamp(record.timestamp),
                record.user,
                record.agent,
                record.session_id,
                record.explanation.trim()
            ));
        }
        output
    }
}

fn parse_record(line: &str, path: &Path, line_number: usize) -> Result<GlossRecord> {
    let parts: Vec<&str> = line.splitn(7, char::is_whitespace).collect();
    if parts.len() != 7
        || parts.iter().take(6).any(|part| part.is_empty())
        || parts[6].trim().is_empty()
    {
        return Err(invalid(
            path,
            format!("invalid record at line {line_number}"),
        ));
    }
    let edit_id = Uuid::parse_str(parts[0]).map_err(|_| {
        GlossError::new(
            ErrorCode::InvalidUuid,
            format!("invalid UUID at line {line_number}"),
        )
        .file(path)
    })?;
    let range = LineRange::parse(parts[1]).map_err(|error| error.file(path))?;
    let timestamp = parts[2].parse::<DateTime<Utc>>().map_err(|_| {
        invalid(
            path,
            format!("invalid record timestamp at line {line_number}"),
        )
    })?;
    if !parts[3..6].iter().all(|value| valid_token(value)) {
        return Err(invalid(
            path,
            format!("metadata must use non-empty tokens at line {line_number}"),
        ));
    }
    Ok(GlossRecord {
        edit_id,
        range,
        timestamp,
        user: parts[3].to_owned(),
        agent: parts[4].to_owned(),
        session_id: parts[5].to_owned(),
        explanation: parts[6].trim().to_owned(),
    })
}

pub fn gloss_path(source: &Path) -> Result<PathBuf> {
    let parent = source.parent().unwrap_or_else(|| Path::new(""));
    let name = source.file_name().ok_or_else(|| {
        GlossError::new(ErrorCode::InvalidFormat, "source path has no file name").file(source)
    })?;
    let mut gloss_name = name.to_os_string();
    gloss_name.push(".gloss");
    Ok(parent.join(".annotations").join(gloss_name))
}

pub fn source_path(gloss: &Path) -> Result<PathBuf> {
    let annotations = gloss
        .parent()
        .ok_or_else(|| invalid(gloss, "gloss path has no parent"))?;
    if annotations.file_name().and_then(|name| name.to_str()) != Some(".annotations") {
        return Err(GlossError::new(
            ErrorCode::OrphanedGloss,
            "gloss is not inside a sibling .annotations directory",
        )
        .file(gloss));
    }
    let parent = annotations.parent().unwrap_or_else(|| Path::new(""));
    let name = gloss
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid(gloss, "gloss path is not UTF-8"))?;
    let source_name = name
        .strip_suffix(".gloss")
        .filter(|name| !name.is_empty())
        .ok_or_else(|| invalid(gloss, "gloss file must end in .gloss"))?;
    Ok(parent.join(source_name))
}

pub fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

fn valid_token(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(char::is_whitespace)
}

fn invalid(path: &Path, message: impl Into<String>) -> GlossError {
    GlossError::new(ErrorCode::InvalidFormat, message).file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_and_gloss_paths_round_trip() {
        let source = Path::new("src/bar/baz.ex");
        let gloss = gloss_path(source).unwrap();
        assert_eq!(gloss, Path::new("src/bar/.annotations/baz.ex.gloss"));
        assert_eq!(source_path(&gloss).unwrap(), source);
    }

    #[test]
    fn parses_and_normalizes_a_gloss() {
        let input = "version: 1\nupdated: 2026-08-28T18:42:11Z\neditor: codex\n\n0198f5cf-4807-7ac3-a42a-938ff9b78220 42:58 2026-08-28T18:41:53Z calvin codex sess_123 Explain the intent.\n";
        let parsed = GlossFile::parse(input, Path::new("foo.gloss")).unwrap();
        assert_eq!(parsed.records[0].range, LineRange { start: 42, end: 58 });
        assert_eq!(parsed.render(), input);
    }
}
