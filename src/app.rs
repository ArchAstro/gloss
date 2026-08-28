use crate::error::{ErrorCode, GlossError, Result};
use crate::format::{gloss_path, source_path, GlossFile, GlossRecord, LineRange};
use crate::git::{diff_hunks_between, ChangeScope, DiffHunk, GitRepo, LifecycleChange};
use crate::harness::{SkillInstallPlan, SkillScope};
use crate::state::{hash, DerivedState};
use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use walkdir::{DirEntry, WalkDir};

pub struct App {
    repo: GitRepo,
}

#[derive(Debug, Default)]
pub struct AddOptions {
    pub user: Option<String>,
    pub agent: Option<String>,
    pub session: Option<String>,
}

#[derive(Debug, Default)]
pub struct UpdateOptions {
    pub editor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LintOptions {
    pub scope: ChangeScope,
    pub fix: bool,
    pub editor: Option<String>,
}

impl Default for LintOptions {
    fn default() -> Self {
        Self {
            scope: ChangeScope::WorkingTree,
            fix: false,
            editor: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CommandOutput {
    #[serde(skip)]
    pub human: String,
    #[serde(flatten)]
    pub json: Value,
}

impl CommandOutput {
    fn new(human: impl Into<String>, json: Value) -> Self {
        Self {
            human: human.into(),
            json,
        }
    }
}

impl App {
    pub fn discover(start: &Path) -> Result<Self> {
        Ok(Self {
            repo: GitRepo::discover(start)?,
        })
    }

    pub fn init(&self, skill_scope: SkillScope) -> Result<CommandOutput> {
        fs::create_dir_all(self.repo.git_dir().join("annotations"))
            .map_err(|error| GlossError::io(error, self.repo.git_dir().join("annotations")))?;
        let skill_plan = SkillInstallPlan::detect(self.repo.root(), skill_scope)?;
        let workflow = self.install_ci_workflow()?;
        let attributes = self.repo.root().join(".gitattributes");
        let rule = "**/.annotations/*.gloss linguist-generated=true";
        let mut contents = fs::read_to_string(&attributes).unwrap_or_default();
        if !contents.lines().any(|line| line.trim() == rule) {
            if !contents.is_empty() && !contents.ends_with('\n') {
                contents.push('\n');
            }
            contents.push_str(rule);
            contents.push('\n');
            fs::write(&attributes, contents).map_err(|error| GlossError::io(error, &attributes))?;
        }
        self.install_hooks()?;
        let skills = skill_plan.install()?;
        let mut setup_files = vec![PathBuf::from(".gitattributes"), workflow.clone()];
        setup_files.extend(skill_plan.project_files());
        let maintenance = self.update(
            &setup_files,
            UpdateOptions {
                editor: Some("gloss".to_owned()),
            },
        )?;
        let skill_count = skills.len();
        Ok(CommandOutput::new(
            format!(
                "Initialized Gloss: installed hooks, CI validation, generated-file handling, setup metadata, and {skill_count} agent skill{} ({} scope).",
                plural(skill_count),
                skill_plan.scope().as_str(),
            ),
            json!({
                "ok": true,
                "initialized": true,
                "hooks_installed": true,
                "ci_installed": true,
                "generated_files_configured": true,
                "workflow": display(&workflow),
                "skill_scope": skill_plan.scope(),
                "skills": skills,
                "maintenance": maintenance.json,
            }),
        ))
    }

    pub fn add(
        &self,
        file: &Path,
        range: LineRange,
        explanation: &str,
        options: AddOptions,
    ) -> Result<CommandOutput> {
        let source = self.repo.relative(file)?;
        let absolute = self.repo.root().join(&source);
        if !absolute.is_file() {
            return Err(
                GlossError::new(ErrorCode::MissingSource, "source file does not exist")
                    .file(&source),
            );
        }
        validate_range_in_file(&absolute, &range)?;
        let hunks = self.repo.diff_hunks(&source)?;
        if !hunks
            .iter()
            .filter_map(DiffHunk::new_range)
            .any(|hunk| hunk.overlaps(&range))
        {
            return Err(GlossError::new(
                ErrorCode::GlossOutsideHunk,
                "range does not overlap a working-tree edit hunk",
            )
            .file(&source)
            .edit("", range.start, range.end));
        }

        let observed_at = now();
        let user = token(
            options
                .user
                .or_else(|| env::var("GLOSS_USER").ok())
                .or_else(|| self.repo.config("user.name")),
            "unknown",
        );
        let agent = token(
            options.agent.or_else(|| env::var("GLOSS_AGENT").ok()),
            "unknown",
        );
        let session = token(
            options.session.or_else(|| env::var("GLOSS_SESSION").ok()),
            "unknown",
        );
        let gloss = gloss_path(&source)?;
        let absolute_gloss = self.repo.root().join(&gloss);
        let mut document = if absolute_gloss.exists() {
            read_gloss(&absolute_gloss, &gloss)?
        } else {
            GlossFile::empty(observed_at, &agent)
        };
        let now = if absolute_gloss.exists() {
            fresh_timestamp(document.updated)
        } else {
            observed_at
        };
        let edit_id = Uuid::now_v7();
        document.updated = now;
        document.editor = agent.clone();
        document.records.push(GlossRecord {
            edit_id,
            range: range.clone(),
            timestamp: now,
            user,
            agent,
            session_id: session,
            explanation: explanation.trim().to_owned(),
        });
        if explanation.trim().is_empty() {
            return Err(
                GlossError::new(ErrorCode::InvalidFormat, "explanation cannot be empty")
                    .file(&source),
            );
        }
        write_gloss(&absolute_gloss, &document)?;
        let source_bytes = fs::read(&absolute).map_err(|error| GlossError::io(error, &source))?;
        let mut state = DerivedState::load(&self.repo)?;
        state.record_file(&self.repo, &source, &source_bytes, now)?;
        state.save(&self.repo)?;

        Ok(CommandOutput::new(
            format!("Added {edit_id} to {} at {range}.", display(&source)),
            json!({"ok": true, "edit_id": edit_id, "file": display(&source), "range": [range.start, range.end], "gloss_file": display(&gloss)}),
        ))
    }

    pub fn lint(&self, paths: &[PathBuf], options: LintOptions) -> Result<CommandOutput> {
        if options.fix {
            if options.scope != ChangeScope::WorkingTree {
                return Err(GlossError::new(
                    ErrorCode::InvalidFormat,
                    "`--fix` only applies to working-tree lint; stage the fixed glosses before using `--staged`",
                ));
            }
            let maintenance = self.update(
                paths,
                UpdateOptions {
                    editor: options.editor.clone(),
                },
            )?;
            let mut lint = self.lint(
                paths,
                LintOptions {
                    fix: false,
                    ..options
                },
            )?;
            lint.human = format!("{}\n{}", maintenance.human, lint.human);
            lint.json = json!({
                "ok": lint.json.get("ok").and_then(Value::as_bool).unwrap_or(false),
                "fixed": maintenance.json,
                "lint": lint.json,
            });
            return Ok(lint);
        }

        let scope = &options.scope;
        let changed = self.repo.changed_paths_in(scope)?;
        let changed_set: HashSet<PathBuf> = changed.iter().cloned().collect();
        let mut glosses = self.resolve_glosses(paths, true)?;
        let state = DerivedState::load(&self.repo)?;
        let history = self.provenance_from_history()?;
        let mut errors = Vec::new();
        let mut seen: HashMap<Uuid, PathBuf> = HashMap::new();

        for source in changed
            .iter()
            .filter(|path| !ignored_source(path))
            .filter(|path| selected(path, paths))
        {
            let gloss = gloss_path(source)?;
            let source_bytes = self.repo.read_file_in(source, scope);
            let gloss_bytes = self.repo.read_file_in(&gloss, scope);
            if source_bytes.is_none() {
                let existed_before = self.previous_file(scope, &gloss).is_some();
                if existed_before && (gloss_bytes.is_some() || !changed_set.contains(&gloss)) {
                    errors.push(
                        GlossError::new(
                            ErrorCode::OrphanedGloss,
                            "deleted source must delete its gloss in the same change",
                        )
                        .file(&gloss),
                    );
                }
                continue;
            }
            let Some(current_gloss) = gloss_bytes else {
                errors.push(
                    GlossError::new(
                        ErrorCode::MissingGloss,
                        "touched source requires a sibling gloss file; run `gloss lint --fix`",
                    )
                    .file(source),
                );
                continue;
            };
            glosses.push(gloss.clone());
            if !changed_set.contains(&gloss) {
                errors.push(
                    GlossError::new(
                        ErrorCode::OutdatedHeader,
                        "touched source requires its gloss header to be updated in the same change",
                    )
                    .file(&gloss),
                );
                continue;
            }
            if let Some(previous) = self.previous_file(scope, &gloss) {
                if let (Ok(current), Ok(previous)) = (
                    parse_gloss_bytes(&current_gloss, &gloss),
                    parse_gloss_bytes(&previous, &gloss),
                ) {
                    if current.updated == previous.updated {
                        errors.push(
                            GlossError::new(
                                ErrorCode::OutdatedHeader,
                                "gloss changed but its updated timestamp was not bumped",
                            )
                            .file(&gloss),
                        );
                    }
                }
            }
        }

        glosses.sort();
        glosses.dedup();

        for gloss in &glosses {
            let source = match source_path(gloss) {
                Ok(path) => path,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };
            let Some(source_bytes) = self.repo.read_file_in(&source, scope) else {
                errors.push(
                    GlossError::new(
                        ErrorCode::MissingSource,
                        "corresponding source file does not exist",
                    )
                    .file(gloss),
                );
                continue;
            };
            let Some(gloss_bytes) = self.repo.read_file_in(gloss, scope) else {
                continue;
            };
            let document = match parse_gloss_bytes(&gloss_bytes, gloss) {
                Ok(document) => document,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };
            if scope == &ChangeScope::WorkingTree {
                if let Some(file_state) = state.file(&source) {
                    if file_state.source_hash != hash(&source_bytes)
                        || file_state.header_updated != document.updated
                    {
                        errors.push(
                            GlossError::new(
                                ErrorCode::OutdatedHeader,
                                "header does not reflect the current source contents",
                            )
                            .file(gloss),
                        );
                    }
                }
            }
            let line_count = source_line_count_bytes(&source_bytes);
            let hunks = self.repo.diff_hunks_in(&source, scope).unwrap_or_default();
            for record in &document.records {
                if record.range.end > line_count.max(1) {
                    errors.push(
                        GlossError::new(
                            ErrorCode::InvalidRange,
                            "range extends past the end of the source file",
                        )
                        .file(gloss)
                        .edit(
                            record.edit_id,
                            record.range.start,
                            record.range.end,
                        ),
                    );
                }
                if let Some(first) = seen.insert(record.edit_id, gloss.clone()) {
                    errors.push(
                        GlossError::new(
                            ErrorCode::DuplicateEditId,
                            format!("edit ID already appears in {}", display(&first)),
                        )
                        .file(gloss)
                        .edit(
                            record.edit_id,
                            record.range.start,
                            record.range.end,
                        ),
                    );
                }
                let edit_key = record.edit_id.to_string();
                let known = history.contains_key(&edit_key);
                let in_current_hunk = hunks
                    .iter()
                    .filter_map(DiffHunk::new_range)
                    .any(|hunk| hunk.overlaps(&record.range));
                if !known && !in_current_hunk {
                    errors.push(
                        GlossError::new(
                            ErrorCode::GlossOutsideHunk,
                            "gloss cannot be associated with a current or historical edit",
                        )
                        .file(gloss)
                        .edit(
                            record.edit_id,
                            record.range.start,
                            record.range.end,
                        ),
                    );
                }
                if let (Some(cached), Some(actual)) =
                    (state.edits.get(&edit_key), history.get(&edit_key))
                {
                    if cached != actual {
                        errors.push(
                            GlossError::new(
                                ErrorCode::StaleGloss,
                                "derived provenance points to a rewritten commit; run `gloss repair`",
                            )
                            .file(gloss)
                            .edit(record.edit_id, record.range.start, record.range.end),
                        );
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(CommandOutput::new(
                format!(
                    "Gloss lint passed ({} file{}).",
                    glosses.len(),
                    plural(glosses.len())
                ),
                json!({"ok": true, "files_checked": glosses.len(), "errors": []}),
            ))
        } else {
            let human = errors
                .iter()
                .map(format_error)
                .collect::<Vec<_>>()
                .join("\n");
            Ok(CommandOutput::new(
                human,
                json!({"ok": false, "files_checked": glosses.len(), "errors": errors}),
            ))
        }
    }

    pub fn update(&self, paths: &[PathBuf], options: UpdateOptions) -> Result<CommandOutput> {
        let mut state = DerivedState::load(&self.repo)?;
        let editor = token(
            options.editor.or_else(|| env::var("GLOSS_AGENT").ok()),
            "unknown",
        );
        let mut updated_files = Vec::new();
        if paths.is_empty() {
            self.apply_lifecycle_changes(&mut state, &mut updated_files)?;
        }
        self.ensure_touched_glosses(paths, &editor, &mut state, &mut updated_files)?;
        let glosses = self.resolve_glosses(paths, false)?;

        for gloss in glosses {
            let source = source_path(&gloss)?;
            let absolute_source = self.repo.root().join(&source);
            if !absolute_source.is_file() {
                return Err(GlossError::new(
                    ErrorCode::MissingSource,
                    "corresponding source file does not exist",
                )
                .file(&gloss));
            }
            let source_bytes = self.repo.read_worktree_file(&source).ok_or_else(|| {
                GlossError::new(ErrorCode::IoError, "cannot read source file").file(&source)
            })?;
            let current_hash = hash(&source_bytes);
            let mut document = read_gloss(&self.repo.root().join(&gloss), &gloss)?;
            let state_changed = state
                .file(&source)
                .is_some_and(|file| file.source_hash != current_hash);
            let first_observation_changed =
                state.file(&source).is_none() && self.repo.file_changed(&source)?;
            if !state_changed && !first_observation_changed {
                state.record_file(&self.repo, &source, &source_bytes, document.updated)?;
                continue;
            }

            let previous_source = state
                .source_snapshot(&self.repo, &source)
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .or_else(|| self.repo.show_head_file(&source));
            if let Some(old_source) = previous_source {
                let current_source = String::from_utf8_lossy(&source_bytes);
                let hunks = diff_hunks_between(&old_source, &current_source);
                if old_source != current_source {
                    for record in &mut document.records {
                        record.range = remap_range(&record.range, &hunks).map_err(|error| {
                            error.file(&gloss).edit(
                                record.edit_id,
                                record.range.start,
                                record.range.end,
                            )
                        })?;
                    }
                }
            }
            let now = fresh_timestamp(document.updated);
            document.updated = now;
            document.editor = editor.clone();
            write_gloss(&self.repo.root().join(&gloss), &document)?;
            state.record_file(&self.repo, &source, &source_bytes, now)?;
            updated_files.push(display(&gloss));
        }
        state.save(&self.repo)?;
        Ok(CommandOutput::new(
            if updated_files.is_empty() {
                "Gloss metadata is already up to date.".to_owned()
            } else {
                format!(
                    "Updated {} gloss file{}.",
                    updated_files.len(),
                    plural(updated_files.len())
                )
            },
            json!({"ok": true, "updated": updated_files}),
        ))
    }

    pub fn repair(&self) -> Result<CommandOutput> {
        let update = self.update(&[], UpdateOptions::default())?;
        let mappings = self.provenance_from_history()?;
        let mut state = DerivedState::load(&self.repo)?;
        state.edits = mappings.clone();
        state.save(&self.repo)?;
        Ok(CommandOutput::new(
            format!(
                "Repaired provenance for {} edit{}; {}",
                mappings.len(),
                plural(mappings.len()),
                update.human.to_lowercase()
            ),
            json!({"ok": true, "mappings": mappings.len(), "maintenance": update.json}),
        ))
    }

    pub fn status(&self) -> Result<CommandOutput> {
        let mut files = Vec::new();
        let mut human = String::new();
        for source in self.repo.changed_paths()? {
            if ignored_source(&source) {
                continue;
            }
            let absolute = self.repo.root().join(&source);
            if !absolute.is_file() {
                continue;
            }
            let hunks = self.repo.diff_hunks(&source)?;
            if hunks.is_empty() {
                continue;
            }
            let gloss = gloss_path(&source)?;
            let records = if self.repo.root().join(&gloss).exists() {
                read_gloss(&self.repo.root().join(&gloss), &gloss)?.records
            } else {
                Vec::new()
            };
            human.push_str(&format!("{}\n", display(&source)));
            let mut hunk_values = Vec::new();
            for hunk in hunks.iter().filter_map(DiffHunk::new_range) {
                let matches: Vec<_> = records
                    .iter()
                    .filter(|record| record.range.overlaps(&hunk))
                    .collect();
                if matches.is_empty() {
                    human.push_str(&format!("  {:<9}              unglossed\n", hunk));
                    hunk_values
                        .push(json!({"range": [hunk.start, hunk.end], "status": "unglossed"}));
                } else {
                    for record in matches {
                        let short = &record.edit_id.to_string()[..12];
                        human.push_str(&format!("  {:<9}  {short}…  glossed\n", hunk));
                        hunk_values.push(json!({"range": [hunk.start, hunk.end], "status": "glossed", "edit_id": record.edit_id}));
                    }
                }
            }
            human.push('\n');
            files.push(json!({"file": display(&source), "hunks": hunk_values}));
        }
        if files.is_empty() {
            human = "Working tree has no changed source hunks.\n".to_owned();
        }
        Ok(CommandOutput::new(
            human.trim_end().to_owned(),
            json!({"ok": true, "files": files}),
        ))
    }

    pub fn hook_install(&self) -> Result<CommandOutput> {
        self.install_hooks()?;
        Ok(CommandOutput::new(
            "Installed Gloss Git hooks.",
            json!({"ok": true, "hooks_installed": true}),
        ))
    }

    pub fn post_commit(&self) -> Result<CommandOutput> {
        self.repair()
    }
    pub fn post_rewrite(&self) -> Result<CommandOutput> {
        self.repair()
    }

    fn install_hooks(&self) -> Result<()> {
        let hooks = self.repo.git_dir().join("hooks");
        fs::create_dir_all(&hooks).map_err(|error| GlossError::io(error, &hooks))?;
        for (name, command) in [
            ("pre-commit", "gloss lint --staged"),
            ("post-commit", "gloss __post-commit"),
            ("post-rewrite", "gloss __post-rewrite \"$@\""),
        ] {
            install_hook(&hooks.join(name), command)?;
        }
        Ok(())
    }

    fn install_ci_workflow(&self) -> Result<PathBuf> {
        const WORKFLOW: &str = r#"# Managed by `gloss init`. Re-run it to update this workflow.
name: Gloss

on:
  pull_request:

permissions:
  contents: read

jobs:
  lint:
    name: Validate gloss metadata
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - name: Install Gloss
        run: cargo install --git https://github.com/ArchAstro/gloss --tag v__GLOSS_VERSION__ --locked gloss
      - name: Validate changed files
        env:
          GLOSS_BASE: ${{ github.event.pull_request.base.sha }}
        run: gloss lint
"#;
        const MARKER: &str = "# Managed by `gloss init`.";

        let workflow = WORKFLOW.replace("__GLOSS_VERSION__", env!("CARGO_PKG_VERSION"));
        let relative = PathBuf::from(".github/workflows/gloss.yml");
        let path = self.repo.root().join(&relative);
        if path.exists() {
            let existing =
                fs::read_to_string(&path).map_err(|error| GlossError::io(error, &relative))?;
            if existing == workflow {
                return Ok(relative);
            }
            if !existing.starts_with(MARKER) {
                return Err(GlossError::new(
                    ErrorCode::AmbiguousRepair,
                    "refusing to overwrite a non-Gloss workflow at .github/workflows/gloss.yml",
                )
                .file(&relative));
            }
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| GlossError::io(error, parent))?;
        }
        fs::write(&path, workflow).map_err(|error| GlossError::io(error, &relative))?;
        Ok(relative)
    }

    fn previous_file(&self, scope: &ChangeScope, path: &Path) -> Option<Vec<u8>> {
        match scope {
            ChangeScope::WorkingTree | ChangeScope::Staged => {
                self.repo.read_file_at_ref("HEAD", path)
            }
            ChangeScope::Base(base) => self.repo.read_file_at_ref(base, path),
        }
    }

    fn apply_lifecycle_changes(
        &self,
        state: &mut DerivedState,
        updated_files: &mut Vec<String>,
    ) -> Result<()> {
        for change in self.repo.lifecycle_changes()? {
            match change {
                LifecycleChange::Rename { from, to } => {
                    let old_gloss = gloss_path(&from)?;
                    let new_gloss = gloss_path(&to)?;
                    let absolute_old = self.repo.root().join(&old_gloss);
                    let absolute_new = self.repo.root().join(&new_gloss);
                    if !absolute_old.exists() {
                        continue;
                    }
                    if absolute_new.exists() {
                        return Err(GlossError::new(
                            ErrorCode::AmbiguousRepair,
                            "both old and new gloss paths exist for a source rename",
                        )
                        .file(&new_gloss));
                    }
                    if let Some(parent) = absolute_new.parent() {
                        fs::create_dir_all(parent)
                            .map_err(|error| GlossError::io(error, parent))?;
                    }
                    fs::rename(&absolute_old, &absolute_new)
                        .map_err(|error| GlossError::io(error, &old_gloss))?;
                    state.remove_file(&from);
                    updated_files.push(display(&new_gloss));
                }
                LifecycleChange::Delete { path } => {
                    let gloss = gloss_path(&path)?;
                    let absolute = self.repo.root().join(&gloss);
                    if absolute.exists() {
                        fs::remove_file(&absolute)
                            .map_err(|error| GlossError::io(error, &gloss))?;
                        updated_files.push(display(&gloss));
                    }
                    state.remove_file(&path);
                }
            }
        }
        Ok(())
    }

    fn ensure_touched_glosses(
        &self,
        paths: &[PathBuf],
        editor: &str,
        state: &mut DerivedState,
        updated_files: &mut Vec<String>,
    ) -> Result<()> {
        for source in self
            .repo
            .changed_paths()?
            .into_iter()
            .filter(|path| !ignored_source(path))
            .filter(|path| selected(path, paths))
        {
            let absolute_source = self.repo.root().join(&source);
            if !absolute_source.is_file() {
                continue;
            }
            let gloss = gloss_path(&source)?;
            let absolute_gloss = self.repo.root().join(&gloss);
            if absolute_gloss.exists() {
                continue;
            }
            let updated = now();
            let document = GlossFile::empty(updated, editor);
            write_gloss(&absolute_gloss, &document)?;
            let source_bytes = self.repo.read_worktree_file(&source).ok_or_else(|| {
                GlossError::new(ErrorCode::IoError, "cannot read source file").file(&source)
            })?;
            state.record_file(&self.repo, &source, &source_bytes, updated)?;
            updated_files.push(display(&gloss));
        }
        Ok(())
    }

    fn resolve_glosses(&self, paths: &[PathBuf], include_misplaced: bool) -> Result<Vec<PathBuf>> {
        let inputs = if paths.is_empty() {
            vec![self.repo.root().to_owned()]
        } else {
            paths
                .iter()
                .map(|path| self.repo.root().join(path))
                .collect()
        };
        let mut result = Vec::new();
        for input in inputs {
            if input.is_dir() {
                for entry in WalkDir::new(&input)
                    .into_iter()
                    .filter_entry(walk_entry)
                    .filter_map(std::result::Result::ok)
                {
                    if entry.file_type().is_file()
                        && entry.path().extension().and_then(|ext| ext.to_str()) == Some("gloss")
                    {
                        let relative = self.repo.relative(entry.path())?;
                        if include_misplaced || source_path(&relative).is_ok() {
                            result.push(relative);
                        }
                    }
                }
            } else {
                let relative = self.repo.relative(&input)?;
                if relative.extension().and_then(|ext| ext.to_str()) == Some("gloss") {
                    result.push(relative);
                } else {
                    let gloss = gloss_path(&relative)?;
                    if self.repo.root().join(&gloss).exists() {
                        result.push(gloss);
                    }
                }
            }
        }
        result.sort();
        result.dedup();
        Ok(result)
    }

    fn provenance_from_history(&self) -> Result<BTreeMap<String, String>> {
        if !self.repo.head_exists() {
            return Ok(BTreeMap::new());
        }
        let output = self.repo.output([
            "log",
            "--reverse",
            "--format=GLOSS_COMMIT:%H",
            "-p",
            "--",
            "*.gloss",
        ])?;
        if !output.status.success() {
            return Err(GlossError::new(
                ErrorCode::GitError,
                String::from_utf8_lossy(&output.stderr).trim(),
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut commit = String::new();
        let mut mappings = BTreeMap::new();
        for line in text.lines() {
            if let Some(sha) = line.strip_prefix("GLOSS_COMMIT:") {
                commit = sha.to_owned();
                continue;
            }
            if line.starts_with("+++") {
                continue;
            }
            if let Some(added) = line.strip_prefix('+') {
                if let Some(id) = added
                    .split_whitespace()
                    .next()
                    .and_then(|value| Uuid::parse_str(value).ok())
                {
                    mappings
                        .entry(id.to_string())
                        .or_insert_with(|| commit.clone());
                }
            }
        }
        Ok(mappings)
    }
}

fn remap_range(range: &LineRange, hunks: &[DiffHunk]) -> Result<LineRange> {
    let mut delta: i64 = 0;
    for hunk in hunks {
        let old_end = if hunk.old_count == 0 {
            hunk.old_start
        } else {
            hunk.old_start + hunk.old_count - 1
        };
        if old_end < range.start {
            delta += hunk.new_count as i64 - hunk.old_count as i64;
            continue;
        }
        if hunk.old_start > range.end {
            break;
        }
        if hunk.old_count != hunk.new_count {
            return Err(GlossError::new(
                ErrorCode::StaleGloss,
                "source changes overlap this gloss and cannot be remapped deterministically",
            ));
        }
    }
    let start = (range.start as i64 + delta) as u32;
    let end = (range.end as i64 + delta) as u32;
    LineRange::new(start, end)
}

fn validate_range_in_file(path: &Path, range: &LineRange) -> Result<()> {
    let count = source_line_count(path)?;
    if range.end > count.max(1) {
        return Err(GlossError::new(
            ErrorCode::InvalidRange,
            format!(
                "range ends at {}, but the file has {count} lines",
                range.end
            ),
        )
        .file(path));
    }
    Ok(())
}

fn source_line_count(path: &Path) -> Result<u32> {
    let input = fs::read_to_string(path).map_err(|error| GlossError::io(error, path))?;
    if input.is_empty() {
        Ok(0)
    } else {
        Ok(input.lines().count() as u32)
    }
}

fn read_gloss(absolute: &Path, display_path: &Path) -> Result<GlossFile> {
    let input =
        fs::read_to_string(absolute).map_err(|error| GlossError::io(error, display_path))?;
    GlossFile::parse(&input, display_path)
}

fn parse_gloss_bytes(input: &[u8], path: &Path) -> Result<GlossFile> {
    let input = std::str::from_utf8(input).map_err(|_| {
        GlossError::new(ErrorCode::InvalidFormat, "gloss file must be UTF-8").file(path)
    })?;
    GlossFile::parse(input, path)
}

fn source_line_count_bytes(input: &[u8]) -> u32 {
    std::str::from_utf8(input)
        .map(|input| {
            if input.is_empty() {
                0
            } else {
                input.lines().count() as u32
            }
        })
        .unwrap_or(0)
}

fn write_gloss(path: &Path, document: &GlossFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| GlossError::io(error, parent))?;
    }
    fs::write(path, document.render()).map_err(|error| GlossError::io(error, path))
}

fn token(value: Option<String>, fallback: &str) -> String {
    value
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join("_"))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

fn now() -> chrono::DateTime<Utc> {
    Utc::now()
}

fn fresh_timestamp(previous: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    std::cmp::max(now(), previous + chrono::Duration::nanoseconds(1))
}

fn display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}
fn ignored_source(path: &Path) -> bool {
    path.components()
        .any(|part| part.as_os_str() == ".annotations")
}
fn selected(source: &Path, paths: &[PathBuf]) -> bool {
    paths.is_empty()
        || paths.iter().any(|path| {
            source == path
                || source.starts_with(path)
                || gloss_path(source).is_ok_and(|gloss| &gloss == path)
        })
}
fn walk_entry(entry: &DirEntry) -> bool {
    entry.file_name() != ".git" && entry.file_name() != "target"
}

fn format_error(error: &GlossError) -> String {
    let file = error
        .file
        .as_deref()
        .map(|path| format!("{path}: "))
        .unwrap_or_default();
    format!("{}{}: {}", file, error.code.as_str(), error.message)
}

fn install_hook(path: &Path, command: &str) -> Result<()> {
    const START: &str = "# gloss:start";
    const END: &str = "# gloss:end";
    let existing = fs::read_to_string(path).unwrap_or_else(|_| "#!/bin/sh\n".to_owned());
    let cleaned = if let Some(start) = existing.find(START) {
        if let Some(end_offset) = existing[start..].find(END) {
            let end = start + end_offset + END.len();
            format!(
                "{}{}",
                &existing[..start],
                existing[end..].trim_start_matches('\n')
            )
        } else {
            existing
        }
    } else {
        existing
    };
    let mut output = cleaned;
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(&format!("{START}\n{command}\n{END}\n"));
    fs::write(path, output).map_err(|error| GlossError::io(error, path))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)
            .map_err(|error| GlossError::io(error, path))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).map_err(|error| GlossError::io(error, path))?;
    }
    Ok(())
}
