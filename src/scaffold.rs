// SPDX-License-Identifier: GPL-2.0-or-later
//! `spawnpkg new`: a ready-to-publish Rust binary project.
use crate::util;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const LICENSE: &str = include_str!("../assets/LICENSE");
const CI: &str = include_str!("../assets/ci.yml");

const CARGO_TOML: &str = r#"[package]
name = "@NAME@"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
description = "@DESC@"
license = "GPL-2.0-or-later"
@REPO@readme = "README.md"

[dependencies]

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
"#;

const MAIN_RS: &str = "// SPDX-License-Identifier: GPL-2.0-or-later\n\nfn main() {\n    println!(\"{} {}\", env!(\"CARGO_PKG_NAME\"), env!(\"CARGO_PKG_VERSION\"));\n}\n";

pub struct NewOpts {
    pub name: String,
    pub desc: String,
    pub dir: PathBuf,
    pub trailer: Option<String>,
}

fn toml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The files of a fresh project: (relative path, contents).
pub fn files(name: &str, desc: &str, repo_url: Option<&str>) -> Vec<(&'static str, String)> {
    let repo = repo_url
        .map(|u| format!("repository = \"{u}\"\n"))
        .unwrap_or_default();
    let cargo = CARGO_TOML
        .replace("@NAME@", name)
        .replace("@DESC@", &toml_escape(desc))
        .replace("@REPO@", &repo);
    vec![
        ("Cargo.toml", cargo),
        ("src/main.rs", MAIN_RS.to_string()),
        ("LICENSE", LICENSE.to_string()),
        (
            "README.md",
            format!(
                "# {name}\n\n{desc}\n\n## Build\n\n    cargo build --release\n\n## License\n\nGPL-2.0-or-later. See `LICENSE`.\n"
            ),
        ),
        (
            "CHANGELOG.md",
            "# Changelog\n\n## 0.1.0\n- Initial version.\n".to_string(),
        ),
        (".gitignore", "/target\n".to_string()),
        (".github/workflows/ci.yml", CI.to_string()),
    ]
}

fn step(root: &Path, prog: &str, args: &[&str]) -> Result<String, String> {
    util::out(Command::new(prog).args(args).current_dir(root))
}

/// Create the project, lock file, PKGBUILD (via cratepkg if installed) and first commit.
pub fn create(o: &NewOpts) -> Result<Vec<String>, String> {
    if o.dir.exists() {
        return Err(format!("{} already exists", o.dir.display()));
    }
    let gh_user = util::out(Command::new("gh").args(["api", "user", "-q", ".login"]))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let repo_url = gh_user.map(|u| format!("https://github.com/{u}/{}", o.name));
    let mut written = Vec::new();
    for (rel, text) in files(&o.name, &o.desc, repo_url.as_deref()) {
        let p = o.dir.join(rel);
        fs::create_dir_all(p.parent().unwrap_or(&o.dir))
            .map_err(|e| format!("cannot create {}: {e}", o.dir.display()))?;
        fs::write(&p, text).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
        written.push(rel.to_string());
    }
    step(&o.dir, "cargo", &["generate-lockfile", "--offline"])?;
    step(&o.dir, "cargo", &["check", "--quiet", "--offline"])?;
    if util::have("cratepkg") {
        util::out(Command::new("cratepkg").arg("init").arg(&o.dir))?;
        written.push("pkg/PKGBUILD".into());
    }
    step(&o.dir, "git", &["init", "-q", "-b", "main"])?;
    step(&o.dir, "git", &["add", "-A"])?;
    let title = format!("{} 0.1.0: initial scaffold", o.name);
    let mut args = vec!["commit", "-q", "-m", title.as_str()];
    if let Some(t) = &o.trailer {
        args.extend(["-m", t.as_str()]);
    }
    step(&o.dir, "git", &args)?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_files() {
        let f = files("demo", "A \"demo\" tool", Some("https://github.com/u/demo"));
        let get = |p: &str| {
            f.iter()
                .find(|(n, _)| *n == p)
                .map(|(_, c)| c.as_str())
                .unwrap()
        };
        let cargo = get("Cargo.toml");
        assert!(
            cargo.contains("name = \"demo\"")
                && cargo.contains("description = \"A \\\"demo\\\" tool\"")
        );
        assert!(cargo.contains("repository = \"https://github.com/u/demo\"\nreadme"));
        assert!(get("LICENSE").contains("GNU GENERAL PUBLIC LICENSE"));
        assert!(get("README.md").starts_with("# demo\n"));
        assert!(
            !files("demo", "d", None)
                .iter()
                .any(|(_, c)| c.contains("repository ="))
        );
        assert_eq!(f.len(), 7);
    }
}
