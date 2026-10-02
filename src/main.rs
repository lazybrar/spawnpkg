// SPDX-License-Identifier: GPL-2.0-or-later
//! spawnpkg: check a project name across ecosystems, scaffold a Rust tool, publish it to GitHub.

mod publish;
mod scaffold;
mod sources;
mod util;

use sources::{Res, State};
use std::path::PathBuf;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "spawnpkg: name check, scaffold and publish for small Rust tools

usage:
  spawnpkg check <name>... [--brief] [--all]
      is the name used on: Arch repos (incl. provides), AUR, crates.io, your PATH (blocking),
      and GitHub, npm, PyPI, Homebrew, Debian (informational)?
      default: verdict + only the clashes   --all: every source
      --brief: one line per name for scripts/AI, e.g. `foo taken:!crates,github ?:npm`
               (`!` = blocking source, `?:` = source could not be checked)
      exit: 0 free everywhere, 1 taken somewhere, 2 usage error
  spawnpkg new <name> [--desc TEXT] [--dir PATH] [--trailer TEXT] [--skip-check] [--allow-taken]
      checks the name, then creates ~/work/projects/small/<name> (override base with $SPAWNPKG_BASE):
      Cargo.toml, src, GPL-2.0-or-later LICENSE, README, CHANGELOG, CI, Cargo.lock,
      pkg/PKGBUILD (if cratepkg is installed) and a first git commit (--trailer adds a trailer line)
  spawnpkg publish [path] --public|--private [--yes] [--skip-tests]
      fmt + tests, then creates the GitHub repo with gh and pushes; visibility is always explicit";

#[derive(Debug, PartialEq)]
enum Cmd {
    Check {
        names: Vec<String>,
        brief: bool,
        all: bool,
    },
    New {
        name: String,
        desc: Option<String>,
        dir: Option<PathBuf>,
        trailer: Option<String>,
        skip_check: bool,
        allow_taken: bool,
    },
    Publish {
        path: PathBuf,
        public: Option<bool>,
        yes: bool,
        skip_tests: bool,
    },
    Help,
    Version,
}

fn parse_args(args: &[String]) -> Result<Cmd, String> {
    let Some(first) = args.first() else {
        return Ok(Cmd::Help);
    };
    match first.as_str() {
        "-h" | "--help" | "help" => return Ok(Cmd::Help),
        "-V" | "--version" => return Ok(Cmd::Version),
        "check" | "new" | "publish" => {}
        other => return Err(format!("unknown command '{other}'")),
    }
    let allowed: &[&str] = match first.as_str() {
        "check" => &["--brief", "--all"],
        "new" => &[
            "--desc",
            "--dir",
            "--trailer",
            "--skip-check",
            "--allow-taken",
        ],
        _ => &["--public", "--private", "--yes", "--skip-tests"],
    };
    let takes_value = ["--desc", "--dir", "--trailer"];
    let (mut pos, mut flags, mut vals) = (Vec::new(), Vec::new(), Vec::new());
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        if let Some(f) = a.strip_prefix("--").map(|_| a.as_str()) {
            if !allowed.contains(&f) {
                return Err(format!("unknown option '{f}' for {first}"));
            }
            if takes_value.contains(&f) {
                vals.push((f, it.next().ok_or(format!("{f} needs a value"))?.clone()));
            } else {
                flags.push(f);
            }
        } else {
            pos.push(a.clone());
        }
    }
    let flag = |f: &str| flags.contains(&f);
    let val = |f: &str| vals.iter().find(|(k, _)| *k == f).map(|(_, v)| v.clone());
    match first.as_str() {
        "check" => {
            if pos.is_empty() {
                return Err("check needs at least one name".into());
            }
            if let Some(bad) = pos.iter().find(|n| !sources::valid_name(n)) {
                return Err(format!(
                    "invalid name '{bad}' (lowercase letters, digits and @._+- only)"
                ));
            }
            Ok(Cmd::Check {
                names: pos,
                brief: flag("--brief"),
                all: flag("--all"),
            })
        }
        "new" => {
            let [name] = pos.as_slice() else {
                return Err("new needs exactly one name".into());
            };
            if !sources::valid_name(name) {
                return Err(format!(
                    "invalid name '{name}' (lowercase letters, digits and @._+- only)"
                ));
            }
            Ok(Cmd::New {
                name: name.clone(),
                desc: val("--desc"),
                dir: val("--dir").map(PathBuf::from),
                trailer: val("--trailer"),
                skip_check: flag("--skip-check"),
                allow_taken: flag("--allow-taken"),
            })
        }
        _ => {
            if pos.len() > 1 {
                return Err("publish takes at most one path".into());
            }
            let public = match (flag("--public"), flag("--private")) {
                (true, false) => Some(true),
                (false, true) => Some(false),
                (true, true) => return Err("choose --public or --private, not both".into()),
                (false, false) => None,
            };
            Ok(Cmd::Publish {
                path: pos.first().map(PathBuf::from).unwrap_or_else(|| ".".into()),
                public,
                yes: flag("--yes"),
                skip_tests: flag("--skip-tests"),
            })
        }
    }
}

fn join(res: &[&Res], mark_hard: bool) -> String {
    res.iter()
        .map(|r| {
            if mark_hard && r.hard {
                format!("!{}", r.source)
            } else {
                r.source.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Print the result for one name; returns true if the name is taken anywhere.
fn report(name: &str, res: &[Res], brief: bool, all: bool) -> bool {
    let taken: Vec<&Res> = res
        .iter()
        .filter(|r| matches!(r.state, State::Taken(_)))
        .collect();
    let unknown: Vec<&Res> = res
        .iter()
        .filter(|r| matches!(r.state, State::Unknown(_)))
        .collect();
    if brief {
        let mut line = format!(
            "{name} {}",
            if taken.is_empty() {
                "free".to_string()
            } else {
                format!("taken:{}", join(&taken, true))
            }
        );
        if !unknown.is_empty() {
            line += &format!(" ?:{}", join(&unknown, false));
        }
        println!("{line}");
        return !taken.is_empty();
    }
    let checked = res.len() - unknown.len();
    if taken.is_empty() {
        println!("{name}: free ({checked}/{} sources checked)", res.len());
    } else {
        let hard = taken.iter().filter(|r| r.hard).count();
        println!(
            "{name}: TAKEN on {} ({hard} blocking, marked !)",
            taken.len()
        );
    }
    for r in res {
        let line = match &r.state {
            State::Free if all => "free".to_string(),
            State::Free => continue,
            State::Taken(d) => format!("{}TAKEN  {d}", if r.hard { "! " } else { "" }),
            State::Unknown(why) => format!("?      {why}"),
        };
        println!("  {:<7} {line}", r.source);
    }
    !taken.is_empty()
}

fn run(cmd: Cmd) -> Result<ExitCode, String> {
    match cmd {
        Cmd::Help => println!("{USAGE}"),
        Cmd::Version => println!("spawnpkg {VERSION}"),
        Cmd::Check { names, brief, all } => {
            if let Some(bad) = names.iter().find(|n| !sources::valid_name(n)) {
                return Err(format!(
                    "invalid name '{bad}' (lowercase letters, digits and @._+- only)"
                ));
            }
            let mut any = false;
            for n in &names {
                any |= report(n, &sources::check(n), brief, all);
            }
            return Ok(if any {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            });
        }
        Cmd::New {
            name,
            desc,
            dir,
            trailer,
            skip_check,
            allow_taken,
        } => {
            if !sources::valid_name(&name) {
                return Err(format!(
                    "invalid name '{name}' (lowercase letters, digits and @._+- only)"
                ));
            }
            if !skip_check {
                let res = sources::check(&name);
                report(&name, &res, true, false);
                let hard: Vec<&Res> = res
                    .iter()
                    .filter(|r| r.hard && matches!(r.state, State::Taken(_)))
                    .collect();
                if !hard.is_empty() && !allow_taken {
                    return Err(format!(
                        "name is taken on a blocking source ({}); pick another or pass --allow-taken",
                        join(&hard, false)
                    ));
                }
            }
            let base = std::env::var("SPAWNPKG_BASE")
                .ok()
                .filter(|b| !b.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var("HOME").unwrap_or_default())
                        .join("work/projects/small")
                });
            let o = scaffold::NewOpts {
                desc: desc.unwrap_or_else(|| format!("{name}: TODO describe")),
                dir: dir.unwrap_or_else(|| base.join(&name)),
                name,
                trailer,
            };
            let files = scaffold::create(&o)?;
            println!(
                "created {} ({} files, committed)",
                o.dir.display(),
                files.len()
            );
            if !files.iter().any(|f| f == "pkg/PKGBUILD") {
                println!(
                    "note: no pkg/PKGBUILD (cratepkg is not installed; run `cratepkg init` later)"
                );
            }
            println!(
                "next: edit src/main.rs and the description in Cargo.toml, then `spawnpkg publish {} --public|--private`",
                o.dir.display()
            );
        }
        Cmd::Publish {
            path,
            public,
            yes,
            skip_tests,
        } => {
            let public = public.ok_or("choose --public or --private")?;
            println!(
                "published {}",
                publish::publish(&path, public, yes, skip_tests)?
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args) {
        Err(e) => {
            eprintln!("spawnpkg: {e} (see spawnpkg --help)");
            ExitCode::from(2)
        }
        Ok(Cmd::Publish { public: None, .. }) => {
            eprintln!("spawnpkg: choose --public or --private (see spawnpkg --help)");
            ExitCode::from(2)
        }
        Ok(cmd) => run(cmd).unwrap_or_else(|e| {
            eprintln!("spawnpkg: {e}");
            ExitCode::from(1)
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Result<Cmd, String> {
        parse_args(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    fn res(items: &[(&'static str, bool, State)]) -> Vec<Res> {
        items
            .iter()
            .map(|(s, h, st)| Res {
                source: s,
                hard: *h,
                state: st.clone(),
            })
            .collect()
    }

    #[test]
    fn args() {
        assert_eq!(a(&[]), Ok(Cmd::Help));
        assert_eq!(
            a(&["check", "x", "y", "--brief"]),
            Ok(Cmd::Check {
                names: vec!["x".into(), "y".into()],
                brief: true,
                all: false
            })
        );
        assert!(
            a(&["check"]).is_err() && a(&["check", "x", "--nope"]).is_err() && a(&["new"]).is_err()
        );
        assert_eq!(
            a(&["new", "x", "--desc", "d", "--skip-check"]),
            Ok(Cmd::New {
                name: "x".into(),
                desc: Some("d".into()),
                dir: None,
                trailer: None,
                skip_check: true,
                allow_taken: false
            })
        );
        assert!(a(&["new", "x", "--desc"]).is_err());
        assert_eq!(
            a(&["publish", "--public"]),
            Ok(Cmd::Publish {
                path: ".".into(),
                public: Some(true),
                yes: false,
                skip_tests: false
            })
        );
        assert_eq!(
            a(&["publish", "p", "--private", "--yes"]),
            Ok(Cmd::Publish {
                path: "p".into(),
                public: Some(false),
                yes: true,
                skip_tests: false
            })
        );
        assert!(a(&["publish", "--public", "--private"]).is_err());
        assert_eq!(
            a(&["publish"]),
            Ok(Cmd::Publish {
                path: ".".into(),
                public: None,
                yes: false,
                skip_tests: false
            })
        );
    }

    #[test]
    fn join_marks_blocking() {
        let r = res(&[
            ("crates", true, State::Taken("x".into())),
            ("npm", false, State::Taken("y".into())),
        ]);
        let refs: Vec<&Res> = r.iter().collect();
        assert_eq!(join(&refs, true), "!crates,npm");
        assert_eq!(join(&refs, false), "crates,npm");
    }
}
