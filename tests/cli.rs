// SPDX-License-Identifier: GPL-2.0-or-later
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn tmp(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spawnpkg"))
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn flags_and_usage_errors() {
    assert!(String::from_utf8_lossy(&run(&["--version"]).stdout).starts_with("spawnpkg 0."));
    assert_eq!(run(&["bogus"]).status.code(), Some(2));
    assert_eq!(run(&["check"]).status.code(), Some(2));
    assert_eq!(run(&["check", "Bad Name"]).status.code(), Some(2));
    assert_eq!(
        run(&["new", "Bad Name", "--skip-check"]).status.code(),
        Some(2)
    );
}

#[test]
fn new_scaffolds_and_commits() {
    let d = tmp("new").join("demo");
    let o = run(&[
        "new",
        "demo",
        "--skip-check",
        "--desc",
        "A demo",
        "--dir",
        d.to_str().unwrap(),
        "--trailer",
        "Co-Authored-By: T <t@example.com>",
    ]);
    assert!(o.status.success(), "{}", err(&o));
    for f in [
        "Cargo.toml",
        "Cargo.lock",
        "src/main.rs",
        "LICENSE",
        "README.md",
        "CHANGELOG.md",
        ".gitignore",
        ".github/workflows/ci.yml",
    ] {
        assert!(d.join(f).exists(), "missing {f}");
    }
    assert!(
        fs::read_to_string(d.join("Cargo.toml"))
            .unwrap()
            .contains("description = \"A demo\"")
    );
    let log = Command::new("git")
        .args(["log", "--format=%B"])
        .current_dir(&d)
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&log.stdout);
    assert!(log.contains("demo 0.1.0: initial scaffold") && log.contains("Co-Authored-By: T"));
    let st = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&d)
        .output()
        .unwrap();
    assert!(
        st.stdout.is_empty(),
        "tree should be clean after the first commit"
    );
    // the scaffold itself builds and passes fmt
    assert!(
        Command::new("cargo")
            .args(["fmt", "--check"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn new_refuses_existing_dir() {
    let d = tmp("exists");
    let o = run(&["new", "demo", "--skip-check", "--dir", d.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("already exists"));
}

#[test]
fn publish_guards() {
    // visibility must be explicit
    assert_eq!(run(&["publish"]).status.code(), Some(2));
    assert_eq!(
        run(&["publish", "--public", "--private"]).status.code(),
        Some(2)
    );
    // not a project / not a git repo
    let d = tmp("pub");
    assert_eq!(
        run(&["publish", d.to_str().unwrap(), "--private", "--yes"])
            .status
            .code(),
        Some(1)
    );
    fs::write(d.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    let o = run(&["publish", d.to_str().unwrap(), "--private", "--yes"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("not a git repository"));
}

#[test]
fn no_github_flag_is_accepted() {
    // flag parsing only: an invalid name still exits 2 before any network access
    assert_eq!(
        run(&["check", "Bad Name", "--no-github"]).status.code(),
        Some(2)
    );
    assert_eq!(
        run(&["new", "Bad Name", "--no-github", "--skip-check"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(run(&["check", "x", "--nope"]).status.code(), Some(2));
}

fn scaffold(name: &str) -> PathBuf {
    let d = tmp(name).join("p");
    let o = run(&["new", "p", "--skip-check", "--dir", d.to_str().unwrap()]);
    assert!(o.status.success(), "{}", err(&o));
    d
}

fn git(d: &PathBuf, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(args)
        .current_dir(d)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn gate_reports_one_line_then_failure_then_fix() {
    let d = scaffold("gate");
    let p = d.to_str().unwrap();
    let o = run(&["gate", p]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("fmt ok | clippy ok | tests ok ("));
    fs::write(d.join("src/main.rs"), "fn main(){println!(\"x\")}\n").unwrap();
    let o = run(&["gate", p]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("fmt FAILED"));
    assert!(run(&["gate", p, "--fix"]).status.success());
}

#[test]
fn release_bumps_commits_and_tags() {
    let d = scaffold("release");
    let p = d.to_str().unwrap();
    let dry = run(&["release", "minor", "-m", "x", p, "--dry-run"]);
    assert!(dry.status.success(), "{}", err(&dry));
    assert!(String::from_utf8_lossy(&dry.stdout).contains("dry run: 0.1.0 -> 0.2.0"));
    assert!(
        fs::read_to_string(d.join("Cargo.toml"))
            .unwrap()
            .contains("version = \"0.1.0\"")
    );

    let o = run(&[
        "release",
        "patch",
        "-m",
        "fix thing",
        p,
        "--no-package",
        "--trailer",
        "Co-Authored-By: T <t@example.com>",
    ]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains("0.1.0 -> 0.1.1"));
    assert!(
        fs::read_to_string(d.join("Cargo.toml"))
            .unwrap()
            .contains("version = \"0.1.1\"")
    );
    assert!(
        fs::read_to_string(d.join("CHANGELOG.md"))
            .unwrap()
            .starts_with("# Changelog\n\n## 0.1.1\n- fix thing\n\n## 0.1.0")
    );
    assert!(
        fs::read_to_string(d.join("Cargo.lock"))
            .unwrap()
            .contains("version = \"0.1.1\"")
    );
    assert!(git(&d, &["tag"]).contains("v0.1.1"));
    let msg = git(&d, &["log", "-1", "--format=%B"]);
    assert!(msg.contains("p 0.1.1: fix thing") && msg.contains("Co-Authored-By: T"));
    assert!(
        git(&d, &["status", "--porcelain"]).is_empty(),
        "release leaves a clean tree"
    );

    // a second release works and the same tag cannot be reused
    assert!(
        run(&["release", "patch", "-m", "again", p, "--no-package"])
            .status
            .success()
    );
    assert!(git(&d, &["tag"]).contains("v0.1.2"));
}

#[test]
fn release_requires_message_and_valid_bump() {
    assert_eq!(run(&["release", "patch"]).status.code(), Some(2));
    assert_eq!(run(&["release", "huge", "-m", "x"]).status.code(), Some(2));
}
