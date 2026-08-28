use crate::error::{ErrorCode, GlossError, Result};
use crate::format::LineRange;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleChange {
    Rename { from: PathBuf, to: PathBuf },
    Delete { path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeScope {
    WorkingTree,
    Staged,
    Base(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
}

impl DiffHunk {
    pub fn new_range(&self) -> Option<LineRange> {
        (self.new_count > 0).then(|| LineRange {
            start: self.new_start,
            end: self.new_start + self.new_count - 1,
        })
    }
}

#[derive(Debug, Clone)]
pub struct GitRepo {
    root: PathBuf,
    git_dir: PathBuf,
}

impl GitRepo {
    pub fn discover(start: &Path) -> Result<Self> {
        let root = run_at(start, ["rev-parse", "--show-toplevel"])?;
        let root = PathBuf::from(root.trim());
        let git_dir_raw = run_at(&root, ["rev-parse", "--git-dir"])?;
        let git_dir = {
            let path = PathBuf::from(git_dir_raw.trim());
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        };
        Ok(Self { root, git_dir })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub fn relative(&self, path: &Path) -> Result<PathBuf> {
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            self.root.join(path)
        };
        absolute
            .strip_prefix(&self.root)
            .map(Path::to_owned)
            .map_err(|_| {
                GlossError::new(
                    ErrorCode::InvalidFormat,
                    "path is outside the Git repository",
                )
                .file(path)
            })
    }

    pub fn head_exists(&self) -> bool {
        self.output(["rev-parse", "--verify", "HEAD"])
            .is_ok_and(|output| output.status.success())
    }

    pub fn head_sha(&self) -> Option<String> {
        self.run(["rev-parse", "HEAD"])
            .ok()
            .map(|value| value.trim().to_owned())
    }

    pub fn is_tracked(&self, path: &Path) -> bool {
        let relative = self.relative(path).unwrap_or_else(|_| path.to_owned());
        self.output_paths(&["ls-files", "--error-unmatch", "--"], &[&relative])
            .is_ok_and(|output| output.status.success())
    }

    pub fn diff_hunks(&self, path: &Path) -> Result<Vec<DiffHunk>> {
        self.diff_hunks_in(path, &ChangeScope::WorkingTree)
    }

    pub fn diff_hunks_in(&self, path: &Path, scope: &ChangeScope) -> Result<Vec<DiffHunk>> {
        let relative = self.relative(path)?;
        let output = match scope {
            ChangeScope::WorkingTree if self.head_exists() && self.is_tracked(&relative) => {
                self.output_paths(&["diff", "--unified=0", "HEAD", "--"], &[&relative])?
            }
            ChangeScope::WorkingTree => {
                let absolute = self.root.join(&relative);
                self.output_paths(
                    &["diff", "--no-index", "--unified=0", "--", "/dev/null"],
                    &[&absolute],
                )?
            }
            ChangeScope::Staged => {
                self.output_paths(&["diff", "--cached", "--unified=0", "--"], &[&relative])?
            }
            ChangeScope::Base(base) => {
                let comparison = format!("{base}...HEAD");
                self.output_paths(&["diff", "--unified=0", &comparison, "--"], &[&relative])?
            }
        };
        if !output.status.success() && output.status.code() != Some(1) {
            return Err(git_failure(output));
        }
        Ok(parse_hunks(&String::from_utf8_lossy(&output.stdout)))
    }

    pub fn file_changed(&self, path: &Path) -> Result<bool> {
        Ok(!self.diff_hunks(path)?.is_empty())
    }

    pub fn file_changed_in(&self, path: &Path, scope: &ChangeScope) -> Result<bool> {
        Ok(!self.diff_hunks_in(path, scope)?.is_empty())
    }

    pub fn show_head_file(&self, path: &Path) -> Option<String> {
        if !self.head_exists() {
            return None;
        }
        let relative = self.relative(path).ok()?;
        let spec = format!("HEAD:{}", relative.to_string_lossy().replace('\\', "/"));
        let output = self.output(["show", &spec]).ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }

    pub fn changed_paths(&self) -> Result<Vec<PathBuf>> {
        self.changed_paths_in(&ChangeScope::WorkingTree)
    }

    pub fn changed_paths_in(&self, scope: &ChangeScope) -> Result<Vec<PathBuf>> {
        if *scope == ChangeScope::WorkingTree {
            return self.working_tree_paths();
        }
        let output = match scope {
            ChangeScope::Staged => self.output(["diff", "--cached", "--name-status", "-z"])?,
            ChangeScope::Base(base) => {
                let comparison = format!("{base}...HEAD");
                self.output(["diff", "--name-status", "-z", &comparison])?
            }
            ChangeScope::WorkingTree => unreachable!(),
        };
        if !output.status.success() {
            return Err(git_failure(output));
        }
        Ok(parse_name_status(&output.stdout))
    }

    fn working_tree_paths(&self) -> Result<Vec<PathBuf>> {
        let output = self.output(["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        if !output.status.success() {
            return Err(git_failure(output));
        }
        let bytes = output.stdout;
        let fields: Vec<&[u8]> = bytes
            .split(|byte| *byte == 0)
            .filter(|field| !field.is_empty())
            .collect();
        let mut paths = Vec::new();
        let mut index = 0;
        while index < fields.len() {
            let field = String::from_utf8_lossy(fields[index]);
            if field.len() < 4 {
                index += 1;
                continue;
            }
            let status = &field[..2];
            paths.push(PathBuf::from(&field[3..]));
            index += if status.contains('R') || status.contains('C') {
                2
            } else {
                1
            };
        }
        Ok(paths)
    }

    pub fn read_file_in(&self, path: &Path, scope: &ChangeScope) -> Option<Vec<u8>> {
        let relative = self.relative(path).ok()?;
        match scope {
            ChangeScope::WorkingTree => std::fs::read(self.root.join(relative)).ok(),
            ChangeScope::Staged => {
                let spec = format!(":{}", relative.to_string_lossy().replace('\\', "/"));
                let output = self.output(["show", &spec]).ok()?;
                output.status.success().then_some(output.stdout)
            }
            ChangeScope::Base(_) => {
                let spec = format!("HEAD:{}", relative.to_string_lossy().replace('\\', "/"));
                let output = self.output(["show", &spec]).ok()?;
                output.status.success().then_some(output.stdout)
            }
        }
    }

    pub fn read_file_at_ref(&self, reference: &str, path: &Path) -> Option<Vec<u8>> {
        let relative = self.relative(path).ok()?;
        let spec = format!(
            "{reference}:{}",
            relative.to_string_lossy().replace('\\', "/")
        );
        let output = self.output(["show", &spec]).ok()?;
        output.status.success().then_some(output.stdout)
    }

    pub fn lifecycle_changes(&self) -> Result<Vec<LifecycleChange>> {
        let output = self.output(["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        if !output.status.success() {
            return Err(git_failure(output));
        }
        let fields: Vec<&[u8]> = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|field| !field.is_empty())
            .collect();
        let mut changes = Vec::new();
        let mut index = 0;
        while index < fields.len() {
            let field = String::from_utf8_lossy(fields[index]);
            if field.len() < 4 {
                index += 1;
                continue;
            }
            let status = &field[..2];
            let path = PathBuf::from(&field[3..]);
            if status.contains('R') && index + 1 < fields.len() {
                changes.push(LifecycleChange::Rename {
                    from: PathBuf::from(String::from_utf8_lossy(fields[index + 1]).as_ref()),
                    to: path,
                });
                index += 2;
            } else {
                if status.contains('D') {
                    changes.push(LifecycleChange::Delete { path });
                }
                index += 1;
            }
        }
        Ok(changes)
    }

    pub fn config(&self, key: &str) -> Option<String> {
        self.run(["config", "--get", key])
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    }

    pub fn run<const N: usize>(&self, args: [&str; N]) -> Result<String> {
        let output = self.output(args)?;
        if !output.status.success() {
            return Err(git_failure(output));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    pub fn output<const N: usize>(&self, args: [&str; N]) -> Result<Output> {
        Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .map_err(|error| GlossError::new(ErrorCode::GitError, error.to_string()))
    }

    fn output_paths(&self, prefix: &[&str], paths: &[&Path]) -> Result<Output> {
        let mut command = Command::new("git");
        command.args(prefix);
        command.args(paths);
        command
            .current_dir(&self.root)
            .output()
            .map_err(|error| GlossError::new(ErrorCode::GitError, error.to_string()))
    }
}

fn run_at<const N: usize>(directory: &Path, args: [&str; N]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()
        .map_err(|error| GlossError::new(ErrorCode::GitError, error.to_string()))?;
    if !output.status.success() {
        return Err(git_failure(output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_failure(output: Output) -> GlossError {
    let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    GlossError::new(
        ErrorCode::GitError,
        if message.is_empty() {
            "Git command failed".to_owned()
        } else {
            message
        },
    )
}

pub fn parse_hunks(diff: &str) -> Vec<DiffHunk> {
    diff.lines()
        .filter_map(|line| {
            let body = line.strip_prefix("@@ -")?;
            let (old, rest) = body.split_once(" +")?;
            let (new, _) = rest.split_once(" @@")?;
            let (old_start, old_count) = parse_span(old)?;
            let (new_start, new_count) = parse_span(new)?;
            Some(DiffHunk {
                old_start,
                old_count,
                new_start,
                new_count,
            })
        })
        .collect()
}

pub fn diff_hunks_between(old: &str, new: &str) -> Vec<DiffHunk> {
    use similar::{DiffTag, TextDiff};
    TextDiff::from_lines(old, new)
        .ops()
        .iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .map(|op| {
            let old = op.old_range();
            let new = op.new_range();
            DiffHunk {
                old_start: old.start as u32 + 1,
                old_count: old.len() as u32,
                new_start: new.start as u32 + 1,
                new_count: new.len() as u32,
            }
        })
        .collect()
}

fn parse_span(value: &str) -> Option<(u32, u32)> {
    match value.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((value.parse().ok()?, 1)),
    }
}

fn parse_name_status(bytes: &[u8]) -> Vec<PathBuf> {
    let fields: Vec<&[u8]> = bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .collect();
    let mut paths = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let status = String::from_utf8_lossy(fields[index]);
        index += 1;
        if index >= fields.len() {
            break;
        }
        if status.starts_with('R') || status.starts_with('C') {
            index += 1;
            if index < fields.len() {
                paths.push(PathBuf::from(
                    String::from_utf8_lossy(fields[index]).as_ref(),
                ));
                index += 1;
            }
        } else {
            paths.push(PathBuf::from(
                String::from_utf8_lossy(fields[index]).as_ref(),
            ));
            index += 1;
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_zero_context_hunks() {
        let diff = "@@ -4,2 +4,3 @@\n@@ -20 +21,0 @@\n";
        assert_eq!(
            parse_hunks(diff),
            vec![
                DiffHunk {
                    old_start: 4,
                    old_count: 2,
                    new_start: 4,
                    new_count: 3
                },
                DiffHunk {
                    old_start: 20,
                    old_count: 1,
                    new_start: 21,
                    new_count: 0
                },
            ]
        );
    }
}
