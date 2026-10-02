// SPDX-License-Identifier: GPL-2.0-or-later
//! `spawnpkg publish`: push a committed project to a new GitHub repo.
use crate::util;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub desc: String,
    pub keywords: Vec<String>,
}

fn quoted(v: &str) -> Option<String> {
    let v = v.trim().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => out.push(chars.next()?),
            c => out.push(c),
        }
    }
    None
}

pub fn read_manifest(toml: &str) -> Option<Manifest> {
    let (mut name, mut desc, mut keywords, mut in_pkg) = (None, String::new(), Vec::new(), false);
    for l in toml.lines().map(str::trim) {
        if l.starts_with('[') {
            in_pkg = l == "[package]";
        } else if in_pkg {
            if let Some((k, v)) = l.split_once('=') {
                match k.trim() {
                    "name" => name = quoted(v),
                    "description" => desc = quoted(v).unwrap_or_default(),
                    "keywords" => {
                        keywords = v.split('"').skip(1).step_by(2).map(String::from).collect()
                    }
                    _ => {}
                }
            }
        }
    }
    Some(Manifest {
        name: name?,
        desc,
        keywords,
    })
}

fn sh(root: &Path, prog: &str, args: &[&str]) -> Result<String, String> {
    util::out(Command::new(prog).args(args).current_dir(root))
}

pub fn publish(path: &Path, public: bool, yes: bool, skip_tests: bool) -> Result<String, String> {
    let root = fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let toml = fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|_| format!("no Cargo.toml in {}", root.display()))?;
    let m = read_manifest(&toml).ok_or("Cargo.toml has no package name")?;
    if !root.join(".git").exists() {
        return Err("not a git repository (run `git init` and commit first)".into());
    }
    if !sh(&root, "git", &["status", "--porcelain"])?
        .trim()
        .is_empty()
    {
        return Err("uncommitted changes; commit first".into());
    }
    if !root.join("LICENSE").exists() {
        return Err("no LICENSE file; add one before publishing".into());
    }
    let user = sh(&root, "gh", &["api", "user", "-q", ".login"])
        .map_err(|e| format!("gh is not logged in? {e}"))?
        .trim()
        .to_string();
    let slug = format!("{user}/{}", m.name);
    if sh(&root, "gh", &["repo", "view", &slug]).is_ok() {
        return Err(format!("github.com/{slug} already exists"));
    }
    if !skip_tests {
        sh(&root, "cargo", &["fmt", "--check"]).map_err(|e| format!("cargo fmt --check: {e}"))?;
        sh(&root, "cargo", &["test", "--quiet"]).map_err(|e| format!("cargo test: {e}"))?;
    }
    let vis = if public { "public" } else { "private" };
    eprintln!("about to create github.com/{slug} ({vis}) and push the current branch");
    if !util::confirm("Publish?", yes) {
        return Err("cancelled".into());
    }
    let flag = if public { "--public" } else { "--private" };
    let source = format!("--source={}", root.display());
    sh(
        &root,
        "gh",
        &[
            "repo",
            "create",
            &slug,
            flag,
            &source,
            "--remote=origin",
            "--push",
            "--description",
            &m.desc,
        ],
    )?;
    if !m.keywords.is_empty() {
        let mut args = vec!["repo", "edit", slug.as_str()];
        for k in &m.keywords {
            args.extend(["--add-topic", k.as_str()]);
        }
        let _ = sh(&root, "gh", &args);
    }
    Ok(format!("https://github.com/{slug} ({vis})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest() {
        let t = "[package]\nname = \"demo\"\ndescription = \"A \\\"demo\\\"\"\nkeywords = [\"a\", \"b-c\"]\n[dependencies]\nname = \"no\"\n";
        assert_eq!(
            read_manifest(t),
            Some(Manifest {
                name: "demo".into(),
                desc: "A \"demo\"".into(),
                keywords: vec!["a".into(), "b-c".into()]
            })
        );
        assert_eq!(read_manifest("[dependencies]\n"), None);
    }
}
