use crate::error::{ErrorCode, GlossError, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const MIN_VERSION: u8 = 1;
pub const CURRENT_VERSION: u8 = 2;
/// Version that introduced the `labels` and `risk` record fields. The declared
/// file version alone decides a row's field count, so readers never have to
/// guess whether trailing prose is metadata.
const LABELS_VERSION: u8 = 2;

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    #[default]
    None,
    Low,
    Medium,
    High,
}

impl Risk {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }
}

impl Display for Risk {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        })
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
    pub labels: Vec<String>,
    pub risk: Risk,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize)]
pub enum GlossLine {
    Record(GlossRecord),
    Opaque(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct GlossFile {
    pub version: u8,
    pub updated: DateTime<Utc>,
    pub editor: String,
    pub headers: Vec<String>,
    pub body: Vec<GlossLine>,
}

impl GlossFile {
    pub fn empty(updated: DateTime<Utc>, editor: impl Into<String>) -> Self {
        Self {
            version: MIN_VERSION,
            updated,
            editor: editor.into(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn parse(input: &str, path: &Path) -> Result<Self> {
        let mut lines = input.lines().enumerate();
        let (_, version_line) = lines
            .next()
            .ok_or_else(|| invalid(path, "missing version header"))?;
        let version = version_line
            .strip_prefix("version: ")
            .ok_or_else(|| invalid(path, "expected `version: <number>`"))?
            .parse::<u8>()
            .map_err(|_| invalid(path, "version must be a number"))?;
        if !(MIN_VERSION..=CURRENT_VERSION).contains(&version) {
            return Err(invalid(
                path,
                format!("unsupported gloss version {version}"),
            ));
        }

        let (_, updated_line) = lines
            .next()
            .ok_or_else(|| invalid(path, "missing updated header"))?;
        let updated = updated_line
            .strip_prefix("updated: ")
            .ok_or_else(|| invalid(path, "expected `updated: <timestamp>`"))?
            .parse::<DateTime<Utc>>()
            .map_err(|_| invalid(path, "updated must be an RFC 3339 timestamp"))?;

        let (_, editor_line) = lines
            .next()
            .ok_or_else(|| invalid(path, "missing editor header"))?;
        let editor = editor_line
            .strip_prefix("editor: ")
            .filter(|value| valid_token(value))
            .ok_or_else(|| invalid(path, "editor must be one non-empty token"))?
            .to_owned();

        let mut headers = Vec::new();
        loop {
            match lines.next() {
                Some((_, "")) => break,
                Some((_, line)) => headers.push(line.to_owned()),
                None => return Err(invalid(path, "header must be followed by a blank line")),
            }
        }

        let mut body = Vec::new();
        for (offset, line) in lines {
            let line_number = offset + 1;
            if line.trim().is_empty() {
                return Err(invalid(
                    path,
                    format!("unexpected blank line at {line_number}"),
                ));
            }
            let first = line.split_whitespace().next().unwrap_or_default();
            if record_shaped(first) {
                body.push(GlossLine::Record(parse_record(
                    line,
                    version,
                    path,
                    line_number,
                )?));
            } else {
                body.push(GlossLine::Opaque(line.to_owned()));
            }
        }
        Ok(Self {
            version,
            updated,
            editor,
            headers,
            body,
        })
    }

    pub fn records(&self) -> impl Iterator<Item = &GlossRecord> {
        self.body.iter().filter_map(|line| match line {
            GlossLine::Record(record) => Some(record),
            GlossLine::Opaque(_) => None,
        })
    }

    pub fn into_records(self) -> impl Iterator<Item = GlossRecord> {
        self.body.into_iter().filter_map(|line| match line {
            GlossLine::Record(record) => Some(record),
            GlossLine::Opaque(_) => None,
        })
    }

    pub fn push_record(&mut self, record: GlossRecord) {
        self.body.push(GlossLine::Record(record));
    }

    pub fn render(&self) -> String {
        // Never downgrade: a file declaring a newer version may hold opaque rows
        // written by that version, and its rows are laid out for it.
        let required = if self.records().any(uses_v2_fields) {
            LABELS_VERSION
        } else {
            MIN_VERSION
        };
        let version = self.version.max(required);
        let mut output = format!(
            "version: {version}\nupdated: {}\neditor: {}\n",
            timestamp(self.updated),
            self.editor
        );
        for header in &self.headers {
            output.push_str(header);
            output.push('\n');
        }
        output.push('\n');
        for line in &self.body {
            match line {
                GlossLine::Opaque(line) => output.push_str(line),
                GlossLine::Record(record) => {
                    output.push_str(&format!(
                        "{} {} {} {} {} {}",
                        record.edit_id,
                        record.range,
                        timestamp(record.timestamp),
                        record.user,
                        record.agent,
                        record.session_id,
                    ));
                    if version >= LABELS_VERSION {
                        let mut labels = record.labels.clone();
                        labels.sort();
                        let labels = if labels.is_empty() {
                            "-".to_owned()
                        } else {
                            labels.join(",")
                        };
                        output.push(' ');
                        output.push_str(&labels);
                        output.push(' ');
                        output.push_str(&record.risk.to_string());
                    }
                    output.push(' ');
                    output.push_str(record.explanation.trim());
                }
            }
            output.push('\n');
        }
        output
    }
}

fn parse_record(line: &str, version: u8, path: &Path, line_number: usize) -> Result<GlossRecord> {
    // The row layout is fixed by the file's declared version, not sniffed from
    // field contents: the trailing explanation is free-form prose and can
    // imitate any metadata shape, so guessing per row either eats the first
    // words of a rationale as fabricated labels/risk or rejects the record
    // outright. `render` promotes every row when it promotes the header, so a
    // v2 file has no v1 rows left to accommodate.
    let v2 = version >= LABELS_VERSION;
    let expected = if v2 { 9 } else { 7 };
    let parts: Vec<&str> = line.splitn(expected, char::is_whitespace).collect();
    if parts.len() != expected
        || parts.iter().take(expected - 1).any(|part| part.is_empty())
        || parts[expected - 1].trim().is_empty()
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
    let (labels, risk, explanation) = if v2 {
        let labels = parse_labels(parts[6])
            .ok_or_else(|| invalid(path, format!("invalid labels at line {line_number}")))?;
        let risk = Risk::parse(parts[7])
            .ok_or_else(|| invalid(path, format!("invalid risk at line {line_number}")))?;
        (labels, risk, parts[8])
    } else {
        (Vec::new(), Risk::None, parts[6])
    };
    Ok(GlossRecord {
        edit_id,
        range,
        timestamp,
        user: parts[3].to_owned(),
        agent: parts[4].to_owned(),
        session_id: parts[5].to_owned(),
        labels,
        risk,
        explanation: explanation.trim().to_owned(),
    })
}

fn parse_labels(value: &str) -> Option<Vec<String>> {
    if value == "-" {
        return Some(Vec::new());
    }
    let labels: Vec<String> = value.split(',').map(str::to_owned).collect();
    let mut seen = HashSet::with_capacity(labels.len());
    if labels.is_empty()
        || labels.iter().any(|label| !valid_label(label))
        || labels.iter().any(|label| !seen.insert(label))
    {
        return None;
    }
    Some(labels)
}

fn valid_label(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        })
}

fn uses_v2_fields(record: &GlossRecord) -> bool {
    !record.labels.is_empty() || record.risk != Risk::None
}

/// Whether a body line's first field is an attempted record UUID.
///
/// Unknown line kinds are round-tripped verbatim, so this must not swallow a
/// *corrupt* UUID: dropping such a record would turn lost provenance into a
/// silent omission from `why` that still passes `lint`. Anything recognizable
/// as an attempted UUID — five hyphen-separated alphanumeric groups of roughly
/// canonical width — is handed to `parse_record`, which reports `InvalidUuid`.
fn record_shaped(value: &str) -> bool {
    const CANONICAL_LEN: usize = 36;
    let groups: Vec<&str> = value.split('-').collect();
    groups.len() == 5
        && value.len().abs_diff(CANONICAL_LEN) <= 4
        && groups
            .iter()
            .all(|group| !group.is_empty() && group.chars().all(|ch| ch.is_ascii_alphanumeric()))
}

pub fn gloss_path(source: &Path) -> Result<PathBuf> {
    let parent = source.parent().unwrap_or_else(|| Path::new(""));
    let name = source.file_name().ok_or_else(|| {
        GlossError::new(ErrorCode::InvalidFormat, "source path has no file name").file(source)
    })?;
    let mut gloss_name = name.to_os_string();
    gloss_name.push(".gloss");
    Ok(parent.join(".gloss").join(gloss_name))
}

pub fn source_path(gloss: &Path) -> Result<PathBuf> {
    let metadata_dir = gloss
        .parent()
        .ok_or_else(|| invalid(gloss, "gloss path has no parent"))?;
    if metadata_dir.file_name().and_then(|name| name.to_str()) != Some(".gloss") {
        return Err(GlossError::new(
            ErrorCode::OrphanedGloss,
            "gloss is not inside a sibling .gloss directory",
        )
        .file(gloss));
    }
    let parent = metadata_dir.parent().unwrap_or_else(|| Path::new(""));
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
        assert_eq!(gloss, Path::new("src/bar/.gloss/baz.ex.gloss"));
        assert_eq!(source_path(&gloss).unwrap(), source);
        assert_eq!(
            source_path(Path::new("src/bar/.annotations/baz.ex.gloss"))
                .unwrap_err()
                .code,
            ErrorCode::OrphanedGloss
        );
    }

    const V1_RECORD: &str = "0198f5cf-4807-7ac3-a42a-938ff9b78220 42:58 2026-08-28T18:41:53Z calvin codex sess_123 Explain the intent.";
    const V2_RECORD: &str = "0198f5cf-4807-7ac3-a42a-938ff9b78221 1:2 2026-08-28T18:41:54Z calvin codex sess_456 z-label,a-label high Risky change.";

    /// The explanation is free-form prose, so it can imitate every metadata
    /// shape the old per-row sniffing keyed off: a word that parses as a risk
    /// in the risk position, an embedded comma, and a leading bare `-`.
    const METADATA_LOOKALIKES: [&str; 3] = [
        "compute high water marks lazily because the source is streamed.",
        "Parsing, validation, and lint stay separate.",
        "- none of the callers depend on ordering.",
    ];

    fn file(version: u8, body: &str) -> String {
        format!("version: {version}\nupdated: 2026-08-28T18:42:11Z\neditor: codex\n\n{body}\n")
    }

    #[test]
    fn parses_and_renders_v1_byte_identically() {
        let input = file(1, V1_RECORD);
        let parsed = GlossFile::parse(&input, Path::new("foo.gloss")).unwrap();
        assert_eq!(
            parsed.records().next().unwrap().range,
            LineRange { start: 42, end: 58 }
        );
        assert_eq!(parsed.render(), input);
    }

    #[test]
    fn rejects_versions_outside_supported_window() {
        for version in [0, 3, 255] {
            assert_eq!(
                GlossFile::parse(&file(version, V1_RECORD), Path::new("foo.gloss"))
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidFormat
            );
        }
        GlossFile::parse(&file(1, V1_RECORD), Path::new("foo.gloss")).unwrap();
        GlossFile::parse(&file(2, V2_RECORD), Path::new("foo.gloss")).unwrap();
    }

    #[test]
    fn round_trips_unknown_headers_and_body_lines() {
        let input = format!(
            "version: 1\nupdated: 2026-08-28T18:42:11Z\neditor: codex\nfuture: value\n\nfuture-record payload\n{V1_RECORD}\nother-kind payload\n"
        );
        let parsed = GlossFile::parse(&input, Path::new("foo.gloss")).unwrap();
        assert_eq!(parsed.records().count(), 1);
        assert_eq!(parsed.render(), input);
    }

    #[test]
    fn parses_and_renders_v2_record() {
        let input = file(2, V2_RECORD);
        let parsed = GlossFile::parse(&input, Path::new("foo.gloss")).unwrap();
        let record = parsed.records().next().unwrap();
        assert_eq!(record.labels, ["z-label", "a-label"]);
        assert_eq!(record.risk, Risk::High);
        assert_eq!(
            parsed.render(),
            file(2, &V2_RECORD.replace("z-label,a-label", "a-label,z-label"))
        );
    }

    #[test]
    fn explanations_that_imitate_metadata_survive_both_versions() {
        for prose in METADATA_LOOKALIKES {
            let v1 = file(1, &V1_RECORD.replace("Explain the intent.", prose));
            let parsed = GlossFile::parse(&v1, Path::new("foo.gloss")).unwrap();
            let record = parsed.records().next().unwrap();
            assert_eq!(record.explanation, prose, "v1 explanation was rewritten");
            assert!(record.labels.is_empty(), "v1 gained fabricated labels");
            assert_eq!(record.risk, Risk::None, "v1 gained a fabricated risk");
            assert_eq!(parsed.render(), v1);

            let v2 = file(2, &V2_RECORD.replace("Risky change.", prose));
            let parsed = GlossFile::parse(&v2, Path::new("foo.gloss")).unwrap();
            let record = parsed.records().next().unwrap();
            assert_eq!(record.explanation, prose, "v2 explanation was rewritten");
            assert_eq!(record.labels, ["z-label", "a-label"]);
            assert_eq!(record.risk, Risk::High);
        }
    }

    /// Promoting the header must promote every row with it, so a file never
    /// declares v2 while carrying rows a v2 reader would misparse.
    #[test]
    fn adding_a_labeled_record_promotes_every_row_and_stays_readable() {
        let input = file(
            1,
            &format!(
                "{}\n{}",
                V1_RECORD.replace("Explain the intent.", METADATA_LOOKALIKES[0]),
                V1_RECORD
                    .replace("938ff9b78220", "938ff9b7822a")
                    .replace("Explain the intent.", METADATA_LOOKALIKES[1]),
            ),
        );
        let mut parsed = GlossFile::parse(&input, Path::new("foo.gloss")).unwrap();
        parsed.push_record(GlossRecord {
            edit_id: Uuid::parse_str("0198f5cf-4807-7ac3-a42a-938ff9b78222").unwrap(),
            range: LineRange::new(60, 61).unwrap(),
            timestamp: "2026-08-28T18:41:55Z".parse().unwrap(),
            user: "calvin".to_owned(),
            agent: "codex".to_owned(),
            session_id: "sess_789".to_owned(),
            labels: vec!["security".to_owned()],
            risk: Risk::High,
            explanation: "New labeled record.".to_owned(),
        });
        let rendered = parsed.render();
        assert!(rendered.starts_with("version: 2\n"), "{rendered}");

        let reparsed = GlossFile::parse(&rendered, Path::new("foo.gloss")).unwrap();
        let records: Vec<_> = reparsed.records().collect();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].explanation, METADATA_LOOKALIKES[0]);
        assert_eq!(records[1].explanation, METADATA_LOOKALIKES[1]);
        assert_eq!(records[2].explanation, "New labeled record.");
        for record in &records[..2] {
            assert!(record.labels.is_empty());
            assert_eq!(record.risk, Risk::None);
        }
        assert_eq!(records[2].labels, ["security"]);
        assert_eq!(records[2].risk, Risk::High);
        assert_eq!(reparsed.render(), rendered, "promotion is not idempotent");
    }

    #[test]
    fn row_layout_follows_the_declared_version() {
        assert_eq!(
            GlossFile::parse(&file(2, V1_RECORD), Path::new("foo.gloss"))
                .unwrap_err()
                .code,
            ErrorCode::InvalidFormat
        );
        assert_eq!(
            GlossFile::parse(&file(1, V2_RECORD), Path::new("foo.gloss"))
                .unwrap()
                .records()
                .next()
                .unwrap()
                .explanation,
            "z-label,a-label high Risky change."
        );
    }

    #[test]
    fn render_never_downgrades_the_declared_version() {
        let input =
            "version: 2\nupdated: 2026-08-28T18:42:11Z\neditor: codex\n\nfuture-record payload\n";
        let parsed = GlossFile::parse(input, Path::new("foo.gloss")).unwrap();
        assert_eq!(parsed.version, 2);
        assert_eq!(parsed.render(), input);
    }

    #[test]
    fn default_v2_fields_round_trip_without_downgrading() {
        let input = file(
            2,
            "0198f5cf-4807-7ac3-a42a-938ff9b78221 1:2 2026-08-28T18:41:54Z calvin codex sess_456 - none Default fields.",
        );
        let parsed = GlossFile::parse(&input, Path::new("foo.gloss")).unwrap();
        let record = parsed.records().next().unwrap();
        assert!(record.labels.is_empty());
        assert_eq!(record.risk, Risk::None);
        assert_eq!(parsed.render(), input);
    }

    #[test]
    fn rejects_invalid_v2_labels_and_risk() {
        for fields in [
            "duplicate,duplicate high Explanation.",
            "not_kebab high Explanation.",
            "valid-label,other critical Explanation.",
            "valid-label high",
        ] {
            assert_eq!(
                GlossFile::parse(
                    &file(2, &format!(
                        "0198f5cf-4807-7ac3-a42a-938ff9b78221 1:2 2026-08-28T18:41:54Z calvin codex sess_456 {fields}"
                    )),
                    Path::new("foo.gloss"),
                )
                .unwrap_err()
                .code,
                ErrorCode::InvalidFormat
            );
        }
    }

    #[test]
    fn rejects_malformed_uuid_records_and_blank_body_lines() {
        let malformed = V1_RECORD.replacen("0198", "gggg", 1);
        assert_eq!(
            GlossFile::parse(&file(1, &malformed), Path::new("foo.gloss"))
                .unwrap_err()
                .code,
            ErrorCode::InvalidUuid
        );
        assert_eq!(
            GlossFile::parse(
                &file(1, &format!("{V1_RECORD}\n\nfuture")),
                Path::new("foo.gloss")
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidFormat
        );
    }

    /// Corrupt provenance must stay a loud parse error. Filing these rows as
    /// opaque would drop them from `why` while `lint` still passed clean.
    #[test]
    fn rejects_corrupt_uuids_instead_of_filing_them_as_opaque() {
        for broken in [
            "0198f5cf-4807-7ac3-a42a-938ff9b7822",
            "0198f5cf-4807-7ac3-a42a-938ff9b782200",
            "0198f5cf-4807-7ac3-a42-938ff9b78220",
            "0198f5cg-4807-7ac3-a42a-938ff9b78220",
        ] {
            let input = file(
                1,
                &V1_RECORD.replace("0198f5cf-4807-7ac3-a42a-938ff9b78220", broken),
            );
            assert_eq!(
                GlossFile::parse(&input, Path::new("foo.gloss"))
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidUuid,
                "{broken} was not rejected"
            );
        }
    }

    #[test]
    fn still_round_trips_lines_that_are_not_record_shaped() {
        for opaque in [
            "future-record payload",
            "other-kind payload",
            "a-b-c-d 1:2 payload",
        ] {
            let input = file(1, &format!("{opaque}\n{V1_RECORD}"));
            let parsed = GlossFile::parse(&input, Path::new("foo.gloss")).unwrap();
            assert_eq!(
                parsed.records().count(),
                1,
                "{opaque} was parsed as a record"
            );
            assert_eq!(parsed.render(), input);
        }
    }

    #[test]
    fn app_style_add_preserves_opaque_lines() {
        let input = file(1, &format!("future-record payload\n{V1_RECORD}"));
        let mut parsed = GlossFile::parse(&input, Path::new("foo.gloss")).unwrap();
        parsed.push_record(GlossRecord {
            edit_id: Uuid::parse_str("0198f5cf-4807-7ac3-a42a-938ff9b78222").unwrap(),
            range: LineRange::new(60, 61).unwrap(),
            timestamp: "2026-08-28T18:41:55Z".parse().unwrap(),
            user: "calvin".to_owned(),
            agent: "codex".to_owned(),
            session_id: "sess_789".to_owned(),
            labels: Vec::new(),
            risk: Risk::None,
            explanation: "Another edit.".to_owned(),
        });
        let rendered = parsed.render();
        assert!(rendered.contains(&format!("future-record payload\n{V1_RECORD}\n")));
        assert!(rendered.ends_with("sess_789 Another edit.\n"));
    }
}
