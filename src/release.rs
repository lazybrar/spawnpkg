// SPDX-License-Identifier: GPL-2.0-or-later
//! `spawnpkg release`: gate, bump version, changelog, lockfile, PKGBUILD + package, commit, tag, push.
use crate::{gate, util};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bump {
    Major,
    Minor,
    Patch,
}

pub fn parse_bump(s: &str) -> Option<Bump> {
    match s {
        "major" => Some(Bump::Major),
        "minor" => Some(Bump::Minor),
        "patch" => Some(Bump::Patch),
        _ => None,
    }
}

pub fn bump(v: &str, b: Bump) -> Result<String, String> {
    let p: Vec<u32> = v
        .split('.')
        .map(|x| {
            x.parse()
                .map_err(|_| format!("version '{v}' is not plain x.y.z"))
        })
        .collect::<Result<_, _>>()?;
    let [major, minor, patch] = p[..] else {
        return Err(format!("version '{v}' is not plain x.y.z"));
    };
    Ok(match b {
        Bump::Major => format!("{}.0.0", major + 1),
        Bump::Minor => format!("{major}.{}.0", minor + 1),
        Bump::Patch => format!("{major}.{minor}.{}", patch + 1),
    })
}

fn quoted(v: &str) -> Option<String> {
    let v = v.trim().strip_prefix('"')?;
    v.split('"').next().map(String::from)
}

/// (crate name, version) from `[package]`.
pub fn read_package(toml: &str) -> Result<(String, String), String> {
    let (mut name, mut version, mut in_pkg) = (None, None, false);
    for l in toml.lines().map(str::trim) {
        if l.starts_with('[') {
            in_pkg = l == "[package]";
        } else if in_pkg {
            let Some((k, v)) = l.split_once('=') else {
                continue;
            };
            match k.trim() {
                "name" => name = quoted(v),
                "version" => version = quoted(v),
                "version.workspace" => {
                    return Err("workspace-inherited versions are not supported".into());
                }
                _ => {}
            }
        }
    }
    Ok((
        name.ok_or("no package name in Cargo.toml")?,
        version.ok_or("no plain package version in Cargo.toml")?,
    ))
}

pub fn set_version(toml: &str, new: &str) -> String {
    let mut in_pkg = false;
    let mut done = false;
    let mut out: Vec<String> = Vec::new();
    for l in toml.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            in_pkg = t == "[package]";
        }
        if in_pkg
            && !done
            && t.starts_with("version")
            && t.split_once('=')
                .is_some_and(|(k, _)| k.trim() == "version")
        {
            out.push(format!("version = \"{new}\""));
            done = true;
        } else {
            out.push(l.to_string());
        }
    }
    out.join("\n") + "\n"
}

pub fn changelog_entry(existing: Option<&str>, version: &str, msg: &str) -> String {
    let entry = format!("## {version}\n- {msg}\n\n");
    match existing {
        None => format!("# Changelog\n\n## {version}\n- {msg}\n"),
        Some(t) => match t.find("\n## ") {
            Some(i) => format!("{}\n{entry}{}", &t[..i], &t[i + 1..]),
            None => format!("{}\n\n{}", t.trim_end(), entry.trim_end()) + "\n",
        },
    }
}

pub struct Opts {
    pub path: PathBuf,
    pub bump: Bump,
    pub message: String,
    pub push: bool,
    pub trailer: Option<String>,
    pub no_package: bool,
    pub skip_gate: bool,
    pub dry_run: bool,
}

fn sh(root: &Path, prog: &str, args: &[&str]) -> Result<String, String> {
    util::out(Command::new(prog).args(args).current_dir(root))
}

/// Settings of an existing PKGBUILD that `cratepkg init --force` would otherwise reset.
fn old_pkgbuild_flags(pkgbuild: &str, crate_name: &str) -> (String, Vec<String>) {
    let pkgname = pkgbuild
        .lines()
        .find_map(|l| l.strip_prefix("pkgname="))
        .unwrap_or(crate_name)
        .trim()
        .to_string();
    let mut flags = Vec::new();
    if pkgname != crate_name.to_lowercase() {
        flags.extend(["--pkgname".to_string(), pkgname.clone()]);
    }
    if !pkgbuild.contains("check()") {
        flags.push("--no-check".into());
    }
    (pkgname, flags)
}

pub fn release(o: &Opts) -> Result<Vec<String>, String> {
    let root = fs::canonicalize(&o.path).map_err(|e| format!("{}: {e}", o.path.display()))?;
    let toml = fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|_| format!("no Cargo.toml in {}", root.display()))?;
    let (name, old) = read_package(&toml)?;
    let new = bump(&old, o.bump)?;
    if !root.join(".git").exists() {
        return Err("not a git repository".into());
    }
    let tag = format!("v{new}");
    if sh(
        &root,
        "git",
        &["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")],
    )
    .is_ok()
    {
        return Err(format!("tag {tag} already exists"));
    }
    let mut lines = Vec::new();
    if o.skip_gate {
        lines.push("gate skipped".to_string());
    } else {
        lines.push(gate::run(&root, false)?);
    }
    if o.dry_run {
        lines.push(format!(
            "dry run: {old} -> {new}, changelog \"{}\", tag {tag}{}",
            o.message,
            if o.push { ", push" } else { "" }
        ));
        return Ok(lines);
    }

    fs::write(root.join("Cargo.toml"), set_version(&toml, &new))
        .map_err(|e| format!("cannot write Cargo.toml: {e}"))?;
    let cl = root.join("CHANGELOG.md");
    fs::write(
        &cl,
        changelog_entry(fs::read_to_string(&cl).ok().as_deref(), &new, &o.message),
    )
    .map_err(|e| format!("cannot write CHANGELOG.md: {e}"))?;
    let _ = sh(&root, "cargo", &["update", "-p", &name, "--offline"]);

    let pkgbuild_path = root.join("pkg/PKGBUILD");
    let mut pkg_note = "no pkg/PKGBUILD".to_string();
    if !o.no_package && pkgbuild_path.exists() {
        if util::have("cratepkg") {
            let old_pb = fs::read_to_string(&pkgbuild_path).unwrap_or_default();
            let (pkgname, flags) = old_pkgbuild_flags(&old_pb, &name);
            let mut init = Command::new("cratepkg");
            init.arg("init")
                .arg(&root)
                .arg("--force")
                .args(&flags)
                .current_dir(&root);
            util::out(&mut init)?;
            for e in fs::read_dir(root.join("pkg"))
                .into_iter()
                .flatten()
                .flatten()
            {
                let f = e.file_name().to_string_lossy().into_owned();
                if f.starts_with(&format!("{pkgname}-{old}-")) && f.contains(".pkg.tar") {
                    let _ = fs::remove_file(e.path());
                }
            }
            let built = util::out(
                Command::new("cratepkg")
                    .arg("build")
                    .arg(&root)
                    .current_dir(&root),
            )?;
            pkg_note = built
                .lines()
                .find_map(|l| l.strip_prefix("package ready (not installed): "))
                .map(|p| format!("package {p}"))
                .unwrap_or_else(|| "package built".into());
        } else {
            pkg_note = "PKGBUILD not updated (cratepkg is not installed)".into();
        }
    } else if o.no_package {
        pkg_note = "package skipped".into();
    }

    // keep target/release in step with the release, so a PATH copy built from it is never stale
    let rebuilt = if o.no_package {
        ""
    } else {
        sh(&root, "cargo", &["build", "--release", "--quiet"])?;
        " | binary rebuilt"
    };

    sh(&root, "git", &["add", "-A"])?;
    let title = format!("{name} {new}: {}", o.message);
    let mut args = vec!["commit", "-q", "-m", title.as_str()];
    if let Some(t) = &o.trailer {
        args.extend(["-m", t.as_str()]);
    }
    sh(&root, "git", &args)?;
    sh(&root, "git", &["tag", &tag])?;
    let sha = sh(&root, "git", &["rev-parse", "--short", "HEAD"])?
        .trim()
        .to_string();
    lines.push(format!(
        "{old} -> {new} | changelog ok | {pkg_note}{rebuilt} | committed {sha} | tag {tag}"
    ));
    if o.push {
        sh(&root, "git", &["push", "origin", "HEAD", &tag])?;
        lines.push("pushed".into());
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bumps() {
        assert_eq!(bump("0.2.9", Bump::Patch).unwrap(), "0.2.10");
        assert_eq!(bump("0.2.9", Bump::Minor).unwrap(), "0.3.0");
        assert_eq!(bump("1.2.3", Bump::Major).unwrap(), "2.0.0");
        assert!(bump("1.2", Bump::Patch).is_err() && bump("1.2.3-rc.1", Bump::Patch).is_err());
        assert_eq!(parse_bump("minor"), Some(Bump::Minor));
        assert_eq!(parse_bump("x"), None);
    }

    #[test]
    fn manifest_edit() {
        let t = "[package]\nname = \"d\"\nversion = \"0.1.0\"\nrust-version = \"1.85\"\n\n[dependencies]\nversion = \"9\"\n";
        assert_eq!(read_package(t).unwrap(), ("d".into(), "0.1.0".into()));
        let n = set_version(t, "0.1.1");
        assert!(
            n.contains("version = \"0.1.1\"\nrust-version = \"1.85\"")
                && n.contains("[dependencies]\nversion = \"9\"")
        );
        assert!(read_package("[package]\nname = \"d\"\nversion.workspace = true\n").is_err());
    }

    #[test]
    fn changelog() {
        assert_eq!(
            changelog_entry(None, "0.1.0", "first"),
            "# Changelog\n\n## 0.1.0\n- first\n"
        );
        let c = changelog_entry(Some("# Changelog\n\n## 0.1.0\n- first\n"), "0.1.1", "fix");
        assert_eq!(c, "# Changelog\n\n## 0.1.1\n- fix\n\n## 0.1.0\n- first\n");
        assert_eq!(
            changelog_entry(Some("# Changelog\n"), "0.1.0", "x"),
            "# Changelog\n\n## 0.1.0\n- x\n"
        );
    }

    #[test]
    fn pkgbuild_flags_are_preserved() {
        let (n, f) = old_pkgbuild_flags("pkgname=foo-git\nbuild() {}\n", "Foo");
        assert_eq!(
            (n.as_str(), f),
            (
                "foo-git",
                vec![
                    "--pkgname".to_string(),
                    "foo-git".into(),
                    "--no-check".into()
                ]
            )
        );
        let (n, f) = old_pkgbuild_flags("pkgname=foo\ncheck() {}\n", "Foo");
        assert_eq!((n.as_str(), f.len()), ("foo", 0));
    }
}
