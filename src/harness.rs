use crate::error::{ErrorCode, GlossError, Result};
use serde::Serialize;
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SKILL: &str = include_str!("../.skills/gloss/SKILL.md");
const OPENAI_METADATA: &str = include_str!("../.skills/gloss/agents/openai.yaml");
const OWNERSHIP_MARKER: &str = "managed-by: gloss";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    Project,
    User,
}

impl SkillScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillInstall {
    pub harness: &'static str,
    pub path: String,
    pub changed: bool,
}

#[derive(Debug, Clone)]
struct Target {
    harness: &'static str,
    directory: PathBuf,
}

pub struct SkillInstallPlan {
    scope: SkillScope,
    root: PathBuf,
    targets: Vec<Target>,
}

impl SkillInstallPlan {
    pub fn detect(project_root: &Path, scope: SkillScope) -> Result<Self> {
        let root = match scope {
            SkillScope::Project => project_root.to_owned(),
            SkillScope::User => home_dir()?,
        };
        let mut targets = Vec::new();
        for harness in detect_harnesses() {
            let directory = match harness {
                "claude" => root.join(".claude/skills/gloss"),
                "codex" => root.join(".codex/skills/gloss"),
                "cursor" => root.join(".cursor/plugins/local/archagents/skills/gloss"),
                "grok" => root.join(".grok/skills/gloss"),
                "rovo" => root.join(".rovodev/skills/archagent-gloss"),
                _ => unreachable!("harness registry is exhaustive"),
            };
            targets.push(Target { harness, directory });
        }
        let plan = Self {
            scope,
            root,
            targets,
        };
        plan.preflight()?;
        Ok(plan)
    }

    pub fn scope(&self) -> SkillScope {
        self.scope
    }

    pub fn install(&self) -> Result<Vec<SkillInstall>> {
        let canonical = self.canonical_directory();
        let canonical_metadata = canonical.join("agents/openai.yaml");
        fs::create_dir_all(
            canonical_metadata
                .parent()
                .expect("canonical metadata has a parent"),
        )
        .map_err(|error| GlossError::io(error, &canonical))?;
        let canonical_changed = write_if_changed(&canonical.join("SKILL.md"), SKILL)?
            | write_if_changed(&canonical_metadata, OPENAI_METADATA)?;

        let mut installs = Vec::new();
        for target in &self.targets {
            let skill_path = target.directory.join("SKILL.md");
            let metadata_path = target.directory.join("agents/openai.yaml");
            fs::create_dir_all(metadata_path.parent().expect("metadata has a parent"))
                .map_err(|error| GlossError::io(error, &target.directory))?;
            let changed = install_link(&self.root, &canonical.join("SKILL.md"), &skill_path)?
                | install_link(&self.root, &canonical_metadata, &metadata_path)?
                | canonical_changed;
            installs.push(SkillInstall {
                harness: target.harness,
                path: display(&target.directory),
                changed,
            });
        }
        Ok(installs)
    }

    pub fn project_files(&self) -> Vec<PathBuf> {
        if self.scope != SkillScope::Project {
            return Vec::new();
        }
        let mut files = vec![
            self.canonical_directory().join("SKILL.md"),
            self.canonical_directory().join("agents/openai.yaml"),
        ];
        files.extend(self.targets.iter().flat_map(|target| {
            [
                target.directory.join("SKILL.md"),
                target.directory.join("agents/openai.yaml"),
            ]
        }));
        files
            .into_iter()
            .filter_map(|path| path.strip_prefix(&self.root).ok().map(Path::to_owned))
            .collect()
    }

    fn preflight(&self) -> Result<()> {
        let canonical = self.canonical_directory();
        preflight_canonical(&canonical.join("SKILL.md"), SKILL)?;
        preflight_canonical(&canonical.join("agents/openai.yaml"), OPENAI_METADATA)?;
        for target in &self.targets {
            let skill_path = target.directory.join("SKILL.md");
            let metadata_path = target.directory.join("agents/openai.yaml");
            preflight_adapter(
                &self.root,
                &self.canonical_directory().join("SKILL.md"),
                &skill_path,
                target.harness,
                |contents| contents.contains(OWNERSHIP_MARKER),
            )?;
            preflight_adapter(
                &self.root,
                &self.canonical_directory().join("agents/openai.yaml"),
                &metadata_path,
                target.harness,
                |contents| contents == OPENAI_METADATA,
            )?;
            if fs::symlink_metadata(&skill_path).is_err()
                && target.directory.exists()
                && fs::read_dir(&target.directory)
                    .map_err(|error| GlossError::io(error, &target.directory))?
                    .next()
                    .is_some()
            {
                return Err(GlossError::new(
                    ErrorCode::AmbiguousRepair,
                    format!(
                        "refusing to overwrite a non-empty unmanaged {} skill directory",
                        target.harness
                    ),
                )
                .file(&target.directory));
            }
        }
        Ok(())
    }

    fn canonical_directory(&self) -> PathBuf {
        self.root.join(".skills/gloss")
    }
}

fn detect_harnesses() -> Vec<&'static str> {
    let mut harnesses = Vec::new();
    if command_exists("claude") {
        harnesses.push("claude");
    }
    if command_exists("codex") {
        harnesses.push("codex");
    }
    if command_exists("cursor") {
        harnesses.push("cursor");
    }
    if command_exists("grok") {
        harnesses.push("grok");
    }
    if command_exists("rovodev") || rovo_via_acli() {
        harnesses.push("rovo");
    }
    harnesses
}

fn command_exists(name: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|directory| {
            let candidate = directory.join(name);
            candidate.is_file() && executable(&candidate)
        })
    })
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(path: &Path) -> bool {
    path.is_file()
}

fn rovo_via_acli() -> bool {
    if !command_exists("acli") {
        return false;
    }
    Command::new("acli")
        .args(["rovodev", "--help"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| {
            GlossError::new(
                ErrorCode::IoError,
                "cannot install user skills because no home directory is configured",
            )
        })
}

fn write_if_changed(path: &Path, contents: &str) -> Result<bool> {
    if fs::read_to_string(path).ok().as_deref() == Some(contents) {
        return Ok(false);
    }
    fs::write(path, contents).map_err(|error| GlossError::io(error, path))?;
    Ok(true)
}

fn preflight_canonical(path: &Path, expected: &str) -> Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if !metadata.file_type().is_file() {
        return Err(GlossError::new(
            ErrorCode::AmbiguousRepair,
            "refusing to replace a non-file in the canonical Gloss skill",
        )
        .file(path));
    }
    let contents = fs::read_to_string(path).map_err(|error| GlossError::io(error, path))?;
    if contents != expected && !contents.contains(OWNERSHIP_MARKER) {
        return Err(GlossError::new(
            ErrorCode::AmbiguousRepair,
            "refusing to overwrite an unmanaged canonical Gloss skill",
        )
        .file(path));
    }
    Ok(())
}

fn preflight_adapter(
    root: &Path,
    source: &Path,
    destination: &Path,
    harness: &str,
    managed: impl FnOnce(&str) -> bool,
) -> Result<()> {
    let metadata = match fs::symlink_metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(GlossError::io(error, destination)),
    };
    if metadata.file_type().is_symlink() {
        let actual =
            fs::read_link(destination).map_err(|error| GlossError::io(error, destination))?;
        let expected = relative_target(root, destination, source)?;
        if actual == expected {
            return Ok(());
        }
    } else if metadata.file_type().is_file() {
        let contents =
            fs::read_to_string(destination).map_err(|error| GlossError::io(error, destination))?;
        if managed(&contents) {
            return Ok(());
        }
    }
    Err(GlossError::new(
        ErrorCode::AmbiguousRepair,
        format!("refusing to overwrite an unmanaged {harness} skill adapter"),
    )
    .file(destination))
}

fn install_link(root: &Path, source: &Path, destination: &Path) -> Result<bool> {
    let target = relative_target(root, destination, source)?;
    if fs::symlink_metadata(destination).is_ok_and(|metadata| {
        metadata.file_type().is_symlink()
            && fs::read_link(destination).ok().as_deref() == Some(target.as_path())
    }) {
        return Ok(false);
    }
    match fs::remove_file(destination) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(GlossError::io(error, destination)),
    }
    create_file_symlink(&target, destination)
        .map_err(|error| GlossError::io(error, destination))?;
    Ok(true)
}

fn relative_target(root: &Path, link: &Path, source: &Path) -> Result<PathBuf> {
    let parent = link.parent().expect("skill adapter has a parent");
    let parent = parent.strip_prefix(root).map_err(|_| {
        GlossError::new(
            ErrorCode::InvalidFormat,
            "skill adapter is outside its scope",
        )
        .file(link)
    })?;
    let source = source.strip_prefix(root).map_err(|_| {
        GlossError::new(
            ErrorCode::InvalidFormat,
            "canonical skill is outside its scope",
        )
        .file(source)
    })?;
    let mut target = PathBuf::new();
    for _ in parent.components() {
        target.push("..");
    }
    target.push(source);
    Ok(target)
}

#[cfg(unix)]
fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

fn display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
