use crate::error::{ErrorCode, GlossError, Result};
use serde::Serialize;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

const GLOSS_PATTERN: &str = "**/.annotations/**/*.gloss";
const IGNORE_RULE: &str = "**/.annotations/*.gloss";
const IGNORE_START: &str = "# gloss:start";
const IGNORE_END: &str = "# gloss:end";
const ZED_DEFAULTS: &[&str] = &[
    "**/.git",
    "**/.svn",
    "**/.hg",
    "**/.jj",
    "**/CVS",
    "**/.DS_Store",
    "**/Thumbs.db",
    "**/.classpath",
    "**/.settings",
];

#[derive(Debug, Clone, Serialize)]
pub struct EditorInstall {
    pub editor: String,
    pub path: String,
    pub changed: bool,
}

struct PlannedFile {
    editor: String,
    relative: PathBuf,
    contents: String,
}

pub struct EditorInstallPlan {
    root: PathBuf,
    files: Vec<PlannedFile>,
}

impl EditorInstallPlan {
    /// Build every output in memory first so a settings conflict cannot leave a
    /// repository half configured.
    pub fn detect(root: &Path) -> Result<Self> {
        let mut files = vec![
            planned_ignore(root)?,
            planned_json(root, ".vscode/settings.json", "vscode_family", merge_vscode)?,
            planned_json(root, ".zed/settings.json", "zed", merge_zed)?,
        ];

        let mut sublime = fs::read_dir(root)
            .map_err(|error| GlossError::io(error, root))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "sublime-project")
            })
            .collect::<Vec<_>>();
        sublime.sort();
        for path in sublime {
            let relative = path
                .strip_prefix(root)
                .expect("root entry is relative to root");
            files.push(planned_json_at(
                root,
                relative,
                "sublime_text",
                merge_sublime,
            )?);
        }

        Ok(Self {
            root: root.to_owned(),
            files,
        })
    }

    pub fn install(&self) -> Result<Vec<EditorInstall>> {
        self.files
            .iter()
            .map(|file| {
                let path = self.root.join(&file.relative);
                let changed = fs::read_to_string(&path).ok().as_deref() != Some(&file.contents);
                if changed {
                    fs::create_dir_all(path.parent().expect("editor file has a parent"))
                        .map_err(|error| GlossError::io(error, &file.relative))?;
                    fs::write(&path, &file.contents)
                        .map_err(|error| GlossError::io(error, &file.relative))?;
                }
                Ok(EditorInstall {
                    editor: file.editor.clone(),
                    path: display(&file.relative),
                    changed,
                })
            })
            .collect()
    }

    pub fn project_files(&self) -> Vec<PathBuf> {
        self.files
            .iter()
            .map(|file| file.relative.clone())
            .collect()
    }
}

fn planned_ignore(root: &Path) -> Result<PlannedFile> {
    let relative = PathBuf::from(".ignore");
    let path = root.join(&relative);
    let mut contents = fs::read_to_string(&path).unwrap_or_default();
    if contents.contains(IGNORE_START) || contents.contains(IGNORE_END) {
        let valid = format!("{IGNORE_START}\n{IGNORE_RULE}\n{IGNORE_END}");
        if !contents.contains(&valid)
            || contents.matches(IGNORE_START).count() != 1
            || contents.matches(IGNORE_END).count() != 1
        {
            return Err(conflict(
                &relative,
                "the managed Gloss block in .ignore was edited",
            ));
        }
    } else if !contents.lines().any(|line| line.trim() == IGNORE_RULE) {
        if !contents.is_empty() && !contents.ends_with('\n') {
            contents.push('\n');
        }
        contents.push_str(&format!("{IGNORE_START}\n{IGNORE_RULE}\n{IGNORE_END}\n"));
    }
    Ok(PlannedFile {
        editor: "portable_ignore".to_owned(),
        relative,
        contents,
    })
}

fn planned_json(
    root: &Path,
    relative: &str,
    editor: &str,
    merge: fn(&mut Value, &Path) -> Result<()>,
) -> Result<PlannedFile> {
    planned_json_at(root, Path::new(relative), editor, merge)
}

fn planned_json_at(
    root: &Path,
    relative: &Path,
    editor: &str,
    merge: fn(&mut Value, &Path) -> Result<()>,
) -> Result<PlannedFile> {
    let path = root.join(relative);
    let mut document = if path.exists() {
        let contents =
            fs::read_to_string(&path).map_err(|error| GlossError::io(error, relative))?;
        serde_json::from_str(&contents).map_err(|error| {
            GlossError::new(
                ErrorCode::InvalidFormat,
                format!("cannot safely merge editor settings: {error}"),
            )
            .file(relative)
        })?
    } else {
        Value::Object(Map::new())
    };
    merge(&mut document, relative)?;
    let mut contents = serde_json::to_string_pretty(&document).map_err(|error| {
        GlossError::new(ErrorCode::InvalidFormat, error.to_string()).file(relative)
    })?;
    contents.push('\n');
    Ok(PlannedFile {
        editor: editor.to_owned(),
        relative: relative.to_owned(),
        contents,
    })
}

fn merge_vscode(document: &mut Value, path: &Path) -> Result<()> {
    let root = object(document, path, "editor settings must be a JSON object")?;
    for key in ["files.exclude", "search.exclude", "files.watcherExclude"] {
        let excludes = root.entry(key).or_insert_with(|| Value::Object(Map::new()));
        let excludes = object(excludes, path, &format!("{key} must be a JSON object"))?;
        match excludes.get(GLOSS_PATTERN) {
            Some(Value::Bool(true)) => {}
            Some(_) => {
                return Err(conflict(
                    path,
                    &format!("{key} already defines a conflicting Gloss exclusion"),
                ))
            }
            None => {
                excludes.insert(GLOSS_PATTERN.to_owned(), Value::Bool(true));
            }
        }
    }
    Ok(())
}

fn merge_zed(document: &mut Value, path: &Path) -> Result<()> {
    let root = object(document, path, "editor settings must be a JSON object")?;
    let new_setting = !root.contains_key("file_scan_exclusions");
    let exclusions = root
        .entry("file_scan_exclusions")
        .or_insert_with(|| Value::Array(Vec::new()));
    let exclusions = exclusions
        .as_array_mut()
        .ok_or_else(|| conflict(path, "file_scan_exclusions must be a JSON array"))?;
    if new_setting {
        exclusions.extend(
            ZED_DEFAULTS
                .iter()
                .map(|item| Value::String((*item).to_owned())),
        );
    }
    if !exclusions
        .iter()
        .any(|item| item.as_str() == Some(GLOSS_PATTERN))
    {
        exclusions.push(Value::String(GLOSS_PATTERN.to_owned()));
    }
    Ok(())
}

fn merge_sublime(document: &mut Value, path: &Path) -> Result<()> {
    let root = object(document, path, "Sublime project must be a JSON object")?;
    let folders = root
        .get_mut("folders")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| conflict(path, "Sublime project must contain a folders array"))?;
    for folder in folders {
        let folder = object(folder, path, "each Sublime folder must be a JSON object")?;
        for key in ["file_exclude_patterns", "index_exclude_patterns"] {
            let patterns = folder
                .entry(key)
                .or_insert_with(|| Value::Array(Vec::new()));
            let patterns = patterns
                .as_array_mut()
                .ok_or_else(|| conflict(path, &format!("{key} must be a JSON array")))?;
            if !patterns.iter().any(|item| item.as_str() == Some("*.gloss")) {
                patterns.push(Value::String("*.gloss".to_owned()));
            }
        }
    }
    Ok(())
}

fn object<'a>(
    value: &'a mut Value,
    path: &Path,
    message: &str,
) -> Result<&'a mut Map<String, Value>> {
    value.as_object_mut().ok_or_else(|| conflict(path, message))
}

fn conflict(path: &Path, message: &str) -> GlossError {
    GlossError::new(ErrorCode::AmbiguousRepair, message).file(path)
}

fn display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
