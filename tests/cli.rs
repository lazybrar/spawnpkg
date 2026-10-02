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
