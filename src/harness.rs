use crate::error::{ErrorCode, GlossError, Result};
use serde::Serialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SKILL: &str = include_str!("../assets/skills/gloss/SKILL.md");
const OPENAI_METADATA: &str = include_str!("../assets/skills/gloss/agents/openai.yaml");
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
    skill: String,
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
            let (directory, skill) = match harness {
                "claude" => (root.join(".claude/skills/gloss"), SKILL.to_owned()),
                "codex" => (root.join(".codex/skills/gloss"), SKILL.to_owned()),
                "cursor" => (
                    root.join(".cursor/plugins/local/archagents/skills/gloss"),
                    SKILL.to_owned(),
                ),
                "rovo" => (root.join(".rovodev/skills/archagent-gloss"), rovo_skill()),
                _ => unreachable!("harness registry is exhaustive"),
            };
            targets.push(Target {
                harness,
                directory,
                skill,
            });
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
        let mut installs = Vec::new();
        for target in &self.targets {
            let skill_path = target.directory.join("SKILL.md");
            let metadata_path = target.directory.join("agents/openai.yaml");
            fs::create_dir_all(metadata_path.parent().expect("metadata has a parent"))
                .map_err(|error| GlossError::io(error, &target.directory))?;
            let changed = write_if_changed(&skill_path, &target.skill)?
                | write_if_changed(&metadata_path, OPENAI_METADATA)?;
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
        self.targets
            .iter()
            .flat_map(|target| {
                [
                    target.directory.join("SKILL.md"),
                    target.directory.join("agents/openai.yaml"),
                ]
            })
            .filter_map(|path| path.strip_prefix(&self.root).ok().map(Path::to_owned))
            .collect()
    }

    fn preflight(&self) -> Result<()> {
        for target in &self.targets {
            let skill_path = target.directory.join("SKILL.md");
            if skill_path.exists() {
                let existing = fs::read_to_string(&skill_path)
                    .map_err(|error| GlossError::io(error, &skill_path))?;
                if !existing.contains(OWNERSHIP_MARKER) {
                    return Err(GlossError::new(
                        ErrorCode::AmbiguousRepair,
                        format!(
                            "refusing to overwrite an unmanaged {} skill",
                            target.harness
                        ),
                    )
                    .file(&skill_path));
                }
            } else if target.directory.exists()
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

fn rovo_skill() -> String {
    SKILL.replacen("name: gloss", "name: archagent-gloss", 1)
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

fn display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
