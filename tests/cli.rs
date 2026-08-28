use assert_cmd::Command;
use predicates::prelude::*;
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
    gloss(&repo).arg("init").assert().success();
    let workflow_path = repo.path().join(".github/workflows/gloss.yml");
    let workflow_before = fs::read_to_string(&workflow_path).unwrap();
    let attribute_gloss = repo.path().join(".annotations/.gitattributes.gloss");
    let workflow_gloss = repo
        .path()
        .join(".github/workflows/.annotations/gloss.yml.gloss");
    let attribute_gloss_before = fs::read_to_string(&attribute_gloss).unwrap();
    let workflow_gloss_before = fs::read_to_string(&workflow_gloss).unwrap();

    gloss(&repo).arg("init").assert().success();

    let attributes = fs::read_to_string(repo.path().join(".gitattributes")).unwrap();
    assert_eq!(attributes.matches("linguist-generated=true").count(), 1);
    let attribute = ProcessCommand::new("git")
        .args([
            "check-attr",
            "linguist-generated",
            "--",
            "src/deep/.annotations/foo.txt.gloss",
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
    assert_eq!(fs::read_to_string(workflow_path).unwrap(), workflow_before);
    assert_eq!(
        fs::read_to_string(attribute_gloss).unwrap(),
        attribute_gloss_before
    );
    assert_eq!(
        fs::read_to_string(workflow_gloss).unwrap(),
        workflow_gloss_before
    );
    gloss(&repo).arg("lint").assert().success();
}

#[test]
fn init_refuses_to_overwrite_a_user_owned_ci_workflow() {
    let repo = Repo::new();
    repo.write(".github/workflows/gloss.yml", "name: My custom workflow\n");

    gloss(&repo)
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

    let annotation =
        fs::read_to_string(repo.path().join("src/.annotations/foo.txt.gloss")).unwrap();
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
    run_git(repo.path(), &["add", "src/.annotations/foo.txt.gloss"]);
    gloss(&repo).args(["lint", "--staged"]).assert().success();

    repo.write("src/.annotations/foo.txt.gloss", "not a gloss\n");
    run_git(repo.path(), &["add", "src/.annotations/foo.txt.gloss"]);
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
    let path = repo.path().join("src/.annotations/foo.txt.gloss");
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
    let second = fs::read_to_string(repo.path().join("src/.annotations/foo.txt.gloss")).unwrap();
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
    assert!(!repo.path().join("src/.annotations/foo.txt.gloss").exists());
    assert!(repo.path().join("lib/.annotations/foo.txt.gloss").exists());

    repo.commit_all("move source");
    run_git(repo.path(), &["rm", "lib/foo.txt"]);
    gloss(&repo).arg("update").assert().success();
    assert!(!repo.path().join("lib/.annotations/foo.txt.gloss").exists());
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
