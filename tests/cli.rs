use assert_cmd::Command;
use predicates::prelude::*;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;
use tempfile::TempDir;

struct Repo {
    dir: TempDir,
}

impl Repo {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.name", "Test User"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        Self { dir }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }
    fn write(&self, path: &str, contents: &str) {
        let path = self.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    fn commit_all(&self, message: &str) {
        run_git(self.path(), &["add", "."]);
        run_git(self.path(), &["commit", "-qm", message]);
    }

    fn harness_path(&self, names: &[&str]) -> OsString {
        let directory = self.path().join(".git/test-harness-bin");
        fs::create_dir_all(&directory).unwrap();
        for name in names {
            let executable = directory.join(name);
            fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let current = std::env::var_os("PATH").unwrap_or_default();
        std::env::join_paths(std::iter::once(directory).chain(std::env::split_paths(&current)))
            .unwrap()
    }
}

fn gloss(repo: &Repo) -> Command {
    let mut command = Command::cargo_bin("gloss").unwrap();
    command.current_dir(repo.path());
    command
}

fn run_git(directory: &Path, args: &[&str]) {
    let output = ProcessCommand::new("git")
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn init_is_idempotent_and_hooks_stay_single() {
    let repo = Repo::new();
    let path = repo.harness_path(&["claude", "codex", "cursor", "grok", "rovodev"]);
    gloss(&repo)
        .env("PATH", &path)
        .arg("init")
        .assert()
        .success();
    let workflow_path = repo.path().join(".github/workflows/gloss.yml");
    let workflow_before = fs::read_to_string(&workflow_path).unwrap();
    let attribute_gloss = repo.path().join(".gloss/.gitattributes.gloss");
    let workflow_gloss = repo.path().join(".github/workflows/.gloss/gloss.yml.gloss");
    let attribute_gloss_before = fs::read_to_string(&attribute_gloss).unwrap();
    let workflow_gloss_before = fs::read_to_string(&workflow_gloss).unwrap();
    let skill_paths = [
        ".claude/skills/gloss/SKILL.md",
        ".codex/skills/gloss/SKILL.md",
        ".cursor/plugins/local/archagents/skills/gloss/SKILL.md",
        ".grok/skills/gloss/SKILL.md",
        ".rovodev/skills/archagent-gloss/SKILL.md",
    ];
    let canonical_skill = repo.path().join(".skills/gloss/SKILL.md");
    assert!(canonical_skill.is_file());
    for skill in skill_paths {
        assert!(repo.path().join(skill).is_file(), "missing {skill}");
        assert!(
            fs::symlink_metadata(repo.path().join(skill))
                .unwrap()
                .file_type()
                .is_symlink(),
            "{skill} must reference the canonical skill"
        );
        assert_eq!(
            fs::read_to_string(repo.path().join(skill)).unwrap(),
            fs::read_to_string(&canonical_skill).unwrap()
        );
        let annotation = Path::new(skill)
            .parent()
            .unwrap()
            .join(".gloss/SKILL.md.gloss");
        assert!(repo.path().join(annotation).is_file());
    }
    let codex_skill_before = fs::read_to_string(repo.path().join(skill_paths[1])).unwrap();
    let ignore_before = fs::read_to_string(repo.path().join(".ignore")).unwrap();
    let vscode_before = fs::read_to_string(repo.path().join(".vscode/settings.json")).unwrap();
    let zed_before = fs::read_to_string(repo.path().join(".zed/settings.json")).unwrap();

    gloss(&repo)
        .env("PATH", &path)
        .arg("init")
        .assert()
        .success();

    let attributes = fs::read_to_string(repo.path().join(".gitattributes")).unwrap();
    assert_eq!(attributes.matches("linguist-generated=true").count(), 1);
    let attribute = ProcessCommand::new("git")
        .args([
            "check-attr",
            "linguist-generated",
            "--",
            "src/deep/.gloss/foo.txt.gloss",
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(String::from_utf8(attribute.stdout)
        .unwrap()
        .ends_with(": true\n"));
    let hook = fs::read_to_string(repo.path().join(".git/hooks/pre-commit")).unwrap();
    assert_eq!(hook.matches("# gloss:start").count(), 1);
    assert_eq!(hook.matches("gloss lint --staged").count(), 1);
    assert!(workflow_before.contains("GLOSS_BASE: ${{ github.event.pull_request.base.sha }}"));
    assert!(workflow_before.contains(
        "cargo install --git https://github.com/ArchAstro/gloss --tag v0.1.0 --locked gloss"
    ));
    assert_eq!(fs::read_to_string(workflow_path).unwrap(), workflow_before);
    assert_eq!(
        fs::read_to_string(attribute_gloss).unwrap(),
        attribute_gloss_before
    );
    assert_eq!(
        fs::read_to_string(workflow_gloss).unwrap(),
        workflow_gloss_before
    );
    assert_eq!(
        fs::read_to_string(repo.path().join(skill_paths[1])).unwrap(),
        codex_skill_before
    );
    assert_eq!(
        fs::read_to_string(repo.path().join(".ignore")).unwrap(),
        ignore_before
    );
    assert_eq!(
        fs::read_to_string(repo.path().join(".vscode/settings.json")).unwrap(),
        vscode_before
    );
    assert_eq!(
        fs::read_to_string(repo.path().join(".zed/settings.json")).unwrap(),
        zed_before
    );
    assert!(repo.path().join(".gloss/.ignore.gloss").is_file());
    assert!(repo
        .path()
        .join(".vscode/.gloss/settings.json.gloss")
        .is_file());
    assert!(repo
        .path()
        .join(".zed/.gloss/settings.json.gloss")
        .is_file());
    gloss(&repo).arg("lint").assert().success();
}

#[test]
fn init_merges_editor_settings_and_preserves_existing_values() {
    let repo = Repo::new();
    repo.write(".ignore", "vendor/\n");
    repo.write(
        ".vscode/settings.json",
        r#"{
  "editor.fontSize": 15,
  "files.exclude": {"**/.cache": true}
}
"#,
    );
    repo.write(
        ".zed/settings.json",
        r#"{"theme":"One Dark","file_scan_exclusions":["**/vendor"]}
"#,
    );
    repo.write(
        "gloss.sublime-project",
        r#"{"folders":[{"path":".","file_exclude_patterns":["*.tmp"]}]}
"#,
    );

    gloss(&repo)
        .args(["--json", "init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"editor\": \"portable_ignore\""))
        .stdout(predicate::str::contains("\"editor\": \"vscode_family\""))
        .stdout(predicate::str::contains("\"editor\": \"zed\""))
        .stdout(predicate::str::contains("\"editor\": \"sublime_text\""));

    let ignore = fs::read_to_string(repo.path().join(".ignore")).unwrap();
    assert!(ignore.starts_with("vendor/\n"));
    assert_eq!(ignore.matches("# gloss:start").count(), 1);
    assert!(ignore.contains("**/.gloss/*.gloss"));

    let vscode: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo.path().join(".vscode/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(vscode["editor.fontSize"], 15);
    assert_eq!(vscode["files.exclude"]["**/.cache"], true);
    for key in ["files.exclude", "search.exclude", "files.watcherExclude"] {
        assert_eq!(vscode[key]["**/.gloss/**/*.gloss"], true);
    }

    let zed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(repo.path().join(".zed/settings.json")).unwrap())
            .unwrap();
    assert_eq!(zed["theme"], "One Dark");
    assert!(zed["file_scan_exclusions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "**/vendor"));
    assert!(zed["file_scan_exclusions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "**/.gloss/**/*.gloss"));

    let sublime: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo.path().join("gloss.sublime-project")).unwrap(),
    )
    .unwrap();
    let folder = &sublime["folders"][0];
    assert!(folder["file_exclude_patterns"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "*.tmp"));
    assert!(folder["file_exclude_patterns"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "*.gloss"));
    assert!(folder["index_exclude_patterns"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "*.gloss"));
}

#[test]
fn init_preserves_zed_defaults_when_it_creates_the_exclusion_setting() {
    let repo = Repo::new();
    repo.write(".zed/settings.json", "{\"theme\":\"One Dark\"}\n");

    gloss(&repo).arg("init").assert().success();

    let zed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(repo.path().join(".zed/settings.json")).unwrap())
            .unwrap();
    let exclusions = zed["file_scan_exclusions"].as_array().unwrap();
    for expected in [
        "**/.git",
        "**/.DS_Store",
        "**/.settings",
        "**/.gloss/**/*.gloss",
    ] {
        assert!(
            exclusions.iter().any(|value| value == expected),
            "missing {expected}"
        );
    }
}

#[test]
fn init_refuses_conflicting_editor_settings_before_writing_setup_files() {
    let repo = Repo::new();
    repo.write(
        ".vscode/settings.json",
        r#"{"files.exclude":{"**/.gloss/**/*.gloss":false}}
"#,
    );

    gloss(&repo)
        .args(["--json", "init"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"code\": \"ambiguous_repair\""));

    assert!(!repo.path().join(".ignore").exists());
    assert!(!repo.path().join(".gitattributes").exists());
    assert!(!repo.path().join(".github/workflows/gloss.yml").exists());
    assert_eq!(
        fs::read_to_string(repo.path().join(".vscode/settings.json")).unwrap(),
        "{\"files.exclude\":{\"**/.gloss/**/*.gloss\":false}}\n"
    );
}

#[test]
fn editor_exclusions_do_not_make_glosses_git_ignored() {
    let repo = Repo::new();
    gloss(&repo).arg("init").assert().success();
    let annotation = ".vscode/.gloss/settings.json.gloss";

    let ignored = ProcessCommand::new("git")
        .args(["check-ignore", "--quiet", "--", annotation])
        .current_dir(repo.path())
        .status()
        .unwrap();
    assert!(!ignored.success());

    run_git(repo.path(), &["add", "."]);
    let tracked = ProcessCommand::new("git")
        .args(["ls-files", "--error-unmatch", annotation])
        .current_dir(repo.path())
        .status()
        .unwrap();
    assert!(tracked.success());
}

#[test]
fn init_user_installs_detected_skills_in_the_home_directory() {
    let repo = Repo::new();
    let home = tempfile::tempdir().unwrap();
    let path = repo.harness_path(&["claude", "codex", "cursor", "grok", "rovodev"]);

    gloss(&repo)
        .env("PATH", path)
        .env("HOME", home.path())
        .args(["--json", "init", "--user"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"skill_scope\": \"user\""));

    let canonical_skill = home.path().join(".skills/gloss/SKILL.md");
    assert!(canonical_skill.is_file());
    for skill in [
        ".claude/skills/gloss/SKILL.md",
        ".codex/skills/gloss/SKILL.md",
        ".cursor/plugins/local/archagents/skills/gloss/SKILL.md",
        ".grok/skills/gloss/SKILL.md",
        ".rovodev/skills/archagent-gloss/SKILL.md",
    ] {
        assert!(home.path().join(skill).is_file(), "missing {skill}");
        assert!(
            fs::symlink_metadata(home.path().join(skill))
                .unwrap()
                .file_type()
                .is_symlink(),
            "{skill} must reference the canonical skill"
        );
        assert!(!repo.path().join(skill).exists());
    }
    assert!(repo.path().join(".gitattributes").is_file());
    assert!(repo.path().join(".github/workflows/gloss.yml").is_file());
}

#[test]
fn init_project_does_not_require_a_home_directory() {
    let repo = Repo::new();
    let path = repo.harness_path(&["codex", "grok", "rovodev"]);

    gloss(&repo)
        .env("PATH", path)
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .args(["init", "--project"])
        .assert()
        .success();

    assert!(repo.path().join(".codex/skills/gloss/SKILL.md").is_file());
    assert!(repo.path().join(".grok/skills/gloss/SKILL.md").is_file());
    assert!(repo.path().join(".skills/gloss/SKILL.md").is_file());
}

#[test]
fn init_migrates_managed_skill_copies_to_canonical_adapters() {
    let repo = Repo::new();
    let path = repo.harness_path(&["codex"]);
    repo.write(
        ".codex/skills/gloss/SKILL.md",
        include_str!("../.skills/gloss/SKILL.md"),
    );

    gloss(&repo)
        .env("PATH", path)
        .arg("init")
        .assert()
        .success();

    let adapter = repo.path().join(".codex/skills/gloss/SKILL.md");
    assert!(fs::symlink_metadata(adapter)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(repo.path().join(".skills/gloss/SKILL.md").is_file());
    gloss(&repo).arg("lint").assert().success();
}

#[test]
fn init_refuses_to_overwrite_an_unmanaged_agent_skill() {
    let repo = Repo::new();
    let path = repo.harness_path(&["claude", "rovodev"]);
    repo.write(
        ".claude/skills/gloss/SKILL.md",
        "---\nname: gloss\n---\nUser-owned skill.\n",
    );

    gloss(&repo)
        .env("PATH", path)
        .args(["--json", "init"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"code\": \"ambiguous_repair\""));
    assert!(!repo.path().join(".gitattributes").exists());
    assert!(!repo.path().join(".github/workflows/gloss.yml").exists());
}

#[test]
fn init_refuses_to_overwrite_a_user_owned_ci_workflow() {
    let repo = Repo::new();
    let path = repo.harness_path(&["rovodev"]);
    repo.write(".github/workflows/gloss.yml", "name: My custom workflow\n");

    gloss(&repo)
        .env("PATH", path)
        .args(["--json", "init"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"code\": \"ambiguous_repair\""));
    assert_eq!(
        fs::read_to_string(repo.path().join(".github/workflows/gloss.yml")).unwrap(),
        "name: My custom workflow\n"
    );
    assert!(!repo.path().join(".gitattributes").exists());
}

#[test]
fn lint_fix_creates_a_header_only_gloss_for_every_touched_file() {
    let repo = Repo::new();
    repo.write("src/foo.txt", "one\n");
    repo.commit_all("baseline");
    repo.write("src/foo.txt", "changed\n");

    gloss(&repo)
        .args(["--json", "lint"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"code\": \"missing_gloss\""));
    gloss(&repo)
        .env("GLOSS_AGENT", "codex")
        .args(["lint", "--fix"])
        .assert()
        .success();

    let annotation = fs::read_to_string(repo.path().join("src/.gloss/foo.txt.gloss")).unwrap();
    assert!(annotation.starts_with("version: 1\nupdated: "));
    assert!(annotation.contains("\neditor: codex\n\n"));
    assert_eq!(annotation.lines().count(), 4);
    gloss(&repo).arg("lint").assert().success();
}

#[test]
fn staged_lint_requires_a_fresh_staged_gloss_and_reads_index_content() {
    let repo = annotated_repo();
    repo.write("src/foo.txt", "one\nchanged again\n");
    run_git(repo.path(), &["add", "src/foo.txt"]);

    gloss(&repo)
        .args(["--json", "lint", "--staged"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"code\": \"outdated_header\""));

    gloss(&repo)
        .env("GLOSS_AGENT", "codex")
        .args(["lint", "--fix"])
        .assert()
        .success();
    run_git(repo.path(), &["add", "src/.gloss/foo.txt.gloss"]);
    gloss(&repo).args(["lint", "--staged"]).assert().success();

    repo.write("src/.gloss/foo.txt.gloss", "not a gloss\n");
    run_git(repo.path(), &["add", "src/.gloss/foo.txt.gloss"]);
    gloss(&repo)
        .args(["--json", "lint", "--staged"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"code\": \"invalid_format\""));
}

#[test]
fn ci_lint_compares_committed_files_to_an_explicit_base() {
    let repo = annotated_repo();
    let base = head(&repo);
    repo.write("src/foo.txt", "one\nci change\n");
    gloss(&repo)
        .env("GLOSS_AGENT", "ci")
        .args(["lint", "--fix"])
        .assert()
        .success();
    repo.commit_all("valid CI change");
    gloss(&repo)
        .args(["lint", "--base", &base])
        .assert()
        .success();

    let invalid_base = head(&repo);
    repo.write("src/foo.txt", "one\nmissing metadata update\n");
    repo.commit_all("invalid CI change");
    gloss(&repo)
        .args(["--json", "lint", "--base", &invalid_base])
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"code\": \"outdated_header\""));
}

#[test]
fn add_rejects_ranges_outside_a_real_hunk() {
    let repo = Repo::new();
    repo.write("src/foo.txt", "one\ntwo\nthree\n");
    repo.commit_all("baseline");
    repo.write("src/foo.txt", "one\nchanged\nthree\n");

    gloss(&repo)
        .args(["--json", "add", "src/foo.txt", "3:3", "Wrong hunk"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"code\": \"gloss_outside_hunk\""));
}

#[test]
fn add_status_lint_and_history_rebuild_work_end_to_end() {
    let repo = Repo::new();
    repo.write("src/foo.txt", "one\ntwo\nthree\n");
    repo.commit_all("baseline");
    repo.write("src/foo.txt", "one\nchanged\nthree\n");

    gloss(&repo)
        .env("GLOSS_USER", "calvin")
        .env("GLOSS_AGENT", "codex")
        .env("GLOSS_SESSION", "sess_test")
        .args(["add", "src/foo.txt", "2:2", "Keep the parser policy-free."])
        .assert()
        .success();
    gloss(&repo)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("glossed"));
    gloss(&repo).arg("lint").assert().success();

    repo.commit_all("annotated edit");
    fs::remove_dir_all(repo.path().join(".git/annotations")).unwrap();
    gloss(&repo).arg("lint").assert().success();
    gloss(&repo)
        .arg("repair")
        .assert()
        .success()
        .stdout(predicate::str::contains("1 edit"));

    let state = fs::read_to_string(repo.path().join(".git/annotations/index.json")).unwrap();
    assert!(!state.contains("annotated edit"));
    assert!(state.contains(&head(&repo)));
}

#[test]
fn update_shifts_ranges_once_when_lines_move() {
    let repo = Repo::new();
    repo.write("src/foo.txt", "one\ntwo\nthree\n");
    repo.commit_all("baseline");
    repo.write("src/foo.txt", "one\nchanged\nthree\n");
    gloss(&repo)
        .env("GLOSS_AGENT", "codex")
        .args(["add", "src/foo.txt", "2:2", "Preserve the new behavior."])
        .assert()
        .success();
    repo.commit_all("annotated edit");

    repo.write("src/foo.txt", "inserted\none\nchanged\nthree\n");
    gloss(&repo)
        .env("GLOSS_AGENT", "codex")
        .arg("update")
        .assert()
        .success();
    let path = repo.path().join("src/.gloss/foo.txt.gloss");
    let first = fs::read_to_string(&path).unwrap();
    assert!(first.lines().any(|line| line.contains(" 3:3 ")));
    gloss(&repo)
        .env("GLOSS_AGENT", "codex")
        .arg("update")
        .assert()
        .success();
    assert_eq!(fs::read_to_string(path).unwrap(), first);

    repo.write("src/foo.txt", "another\ninserted\none\nchanged\nthree\n");
    gloss(&repo)
        .env("GLOSS_AGENT", "codex")
        .arg("update")
        .assert()
        .success();
    let second = fs::read_to_string(repo.path().join("src/.gloss/foo.txt.gloss")).unwrap();
    assert!(second.lines().any(|line| line.contains(" 4:4 ")));
}

#[test]
fn update_moves_and_deletes_glosses_with_sources() {
    let repo = Repo::new();
    repo.write("src/foo.txt", "one\ntwo\n");
    repo.commit_all("baseline");
    repo.write("src/foo.txt", "one\nchanged\n");
    gloss(&repo)
        .env("GLOSS_AGENT", "codex")
        .args(["add", "src/foo.txt", "2:2", "Document the behavior."])
        .assert()
        .success();
    repo.commit_all("annotated edit");

    fs::create_dir_all(repo.path().join("lib")).unwrap();
    run_git(repo.path(), &["mv", "src/foo.txt", "lib/foo.txt"]);
    gloss(&repo)
        .env("GLOSS_AGENT", "codex")
        .arg("update")
        .assert()
        .success();
    assert!(!repo.path().join("src/.gloss/foo.txt.gloss").exists());
    assert!(repo.path().join("lib/.gloss/foo.txt.gloss").exists());

    repo.commit_all("move source");
    run_git(repo.path(), &["rm", "lib/foo.txt"]);
    gloss(&repo).arg("update").assert().success();
    assert!(!repo.path().join("lib/.gloss/foo.txt.gloss").exists());
}

fn head(repo: &Repo) -> String {
    let output = ProcessCommand::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn annotated_repo() -> Repo {
    let repo = Repo::new();
    repo.write("src/foo.txt", "one\ntwo\n");
    repo.commit_all("baseline");
    repo.write("src/foo.txt", "one\nchanged\n");
    gloss(&repo)
        .env("GLOSS_AGENT", "codex")
        .args(["add", "src/foo.txt", "2:2", "Capture the reason."])
        .assert()
        .success();
    repo.commit_all("annotated edit");
    repo
}
