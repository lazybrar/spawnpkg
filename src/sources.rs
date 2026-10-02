// SPDX-License-Identifier: GPL-2.0-or-later
//! Is a name already used? One probe per ecosystem, all run in parallel.
use crate::util;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::thread;

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Free,
    Taken(String),
    Unknown(String),
}

#[derive(Debug)]
pub struct Res {
    pub source: &'static str,
    /// a clash here blocks packaging or `cargo install` (arch repos, AUR, crates.io, local PATH)
    pub hard: bool,
    pub state: State,
}

pub const SOURCES: [(&str, bool); 9] = [
    ("arch", true),
    ("aur", true),
    ("crates", true),
    ("path", true),
    ("github", false),
    ("npm", false),
    ("pypi", false),
    ("brew", false),
    ("debian", false),
];

pub fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "@._+-".contains(c))
        && !n.starts_with(['-', '.'])
}

pub fn check(name: &str) -> Vec<Res> {
    thread::scope(|s| {
        let handles: Vec<_> = SOURCES
            .iter()
            .map(|&(source, hard)| (source, hard, s.spawn(move || probe(source, name))))
            .collect();
        handles
            .into_iter()
            .map(|(source, hard, h)| Res {
                source,
                hard,
                state: h
                    .join()
                    .unwrap_or_else(|_| State::Unknown("probe panicked".into())),
            })
            .collect()
    })
}

fn probe(source: &str, name: &str) -> State {
    let e = util::enc(name);
    match source {
        "arch" => arch(name),
        "aur" => aur(&e),
        "crates" => crates(&e),
        "path" => path(name),
        "github" => github(name),
        "npm" => by_status(
            &format!("https://registry.npmjs.org/{e}/latest"),
            "npm package",
        ),
        "pypi" => by_status(&format!("https://pypi.org/pypi/{e}/json"), "PyPI project"),
        "brew" => brew(&e),
        "debian" => debian(&e),
        _ => State::Unknown("no probe".into()),
    }
}

fn from_status(code: u16, taken: &str) -> State {
    match code {
        200 => State::Taken(taken.into()),
        404 => State::Free,
        c => State::Unknown(format!("HTTP {c}")),
    }
}

fn by_status(url: &str, what: &str) -> State {
    match util::status(url) {
        Ok(c) => from_status(c, what),
        Err(e) => State::Unknown(e),
    }
}

pub fn str_field<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    let start = s.find(&format!("\"{key}\":\""))? + key.len() + 4;
    s[start..].split('"').next()
}

pub fn num_field(s: &str, key: &str) -> Option<u64> {
    let start = s.find(&format!("\"{key}\":"))? + key.len() + 3;
    let digits: String = s[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

fn short(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}...")
    } else {
        t
    }
}

/// Official repos via the local sync dbs; `-Sddp` also resolves `provides`.
fn arch(name: &str) -> State {
    if !util::have("pacman") {
        return match util::body(&format!(
            "https://archlinux.org/packages/search/json/?name={}",
            util::enc(name)
        )) {
            Ok((200, b)) if b.contains("\"results\":[]") => State::Free,
            Ok((200, _)) => State::Taken("official Arch package".into()),
            Ok((c, _)) => State::Unknown(format!("HTTP {c}")),
            Err(e) => State::Unknown(e),
        };
    }
    let o = Command::new("pacman")
        .args(["-Sddp", "--print-format", "%n", name])
        .stdin(Stdio::null())
        .output();
    match o {
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stdout);
            let err = String::from_utf8_lossy(&o.stderr);
            if err.contains("target not found") {
                State::Free
            } else if o.status.success() {
                match out.lines().next() {
                    Some(p) if p != name => State::Taken(format!("provided by {p} (pacman repos)")),
                    _ => State::Taken("in the pacman repos".into()),
                }
            } else {
                State::Unknown(err.lines().next().unwrap_or("pacman failed").to_string())
            }
        }
        Err(e) => State::Unknown(e.to_string()),
    }
}

fn aur(e: &str) -> State {
    match util::body(&format!("https://aur.archlinux.org/rpc/v5/info?arg[]={e}")) {
        Ok((200, b)) if num_field(&b, "resultcount") == Some(0) => State::Free,
        Ok((200, b)) => State::Taken(format!(
            "AUR package, {} votes",
            num_field(&b, "NumVotes").unwrap_or(0)
        )),
        Ok((c, _)) => State::Unknown(format!("HTTP {c}")),
        Err(e) => State::Unknown(e),
    }
}

fn crates(e: &str) -> State {
    match util::body(&format!("https://crates.io/api/v1/crates/{e}")) {
        Ok((200, b)) => State::Taken(match str_field(&b, "description") {
            Some(d) if !d.is_empty() => format!("crate: {}", short(d, 60)),
            _ => "crates.io crate".into(),
        }),
        Ok((404, _)) => State::Free,
        Ok((c, _)) => State::Unknown(format!("HTTP {c}")),
        Err(e) => State::Unknown(e),
    }
}

fn path(name: &str) -> State {
    let hit = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(|d| std::path::Path::new(d).join(name))
        .find(|p| {
            p.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        });
    match hit {
        Some(p) => State::Taken(format!("installed at {}", p.display())),
        None => State::Free,
    }
}

/// Repos named exactly `name`, via `gh api` when available (authenticated rate limit), else plain curl.
fn github(name: &str) -> State {
    let q = format!("{name} in:name");
    let raw = if util::have("gh") {
        util::out(
            Command::new("gh")
                .args(["api", "-X", "GET", "search/repositories", "-f"])
                .arg(format!("q={q}"))
                .args(["-f", "per_page=30"]),
        )
    } else {
        match util::body(&format!(
            "https://api.github.com/search/repositories?q={}&per_page=30",
            q.replace(' ', "+")
        )) {
            Ok((200, b)) => Ok(b),
            Ok((c, _)) => Err(format!("HTTP {c}")),
            Err(e) => Err(e),
        }
    };
    match raw {
        Ok(b) => github_exact(&b, name),
        Err(e) => State::Unknown(short(&e, 80)),
    }
}

pub fn github_exact(json: &str, name: &str) -> State {
    let mut hits: Vec<(&str, u64)> = json
        .split("\"full_name\":\"")
        .skip(1)
        .filter_map(|chunk| {
            let full = chunk.split('"').next()?;
            let repo = full.rsplit('/').next()?;
            repo.eq_ignore_ascii_case(name)
                .then(|| (full, num_field(chunk, "stargazers_count").unwrap_or(0)))
        })
        .collect();
    hits.sort_by_key(|h| std::cmp::Reverse(h.1));
    match hits.first() {
        None => State::Free,
        Some((full, stars)) => {
            let more = if hits.len() > 1 {
                format!(" +{} more", hits.len() - 1)
            } else {
                String::new()
            };
            State::Taken(format!("{full} ({stars} stars){more}"))
        }
    }
}

fn brew(e: &str) -> State {
    for (kind, url) in [("formula", "formula"), ("cask", "cask")] {
        match util::status(&format!("https://formulae.brew.sh/api/{url}/{e}.json")) {
            Ok(200) => return State::Taken(format!("Homebrew {kind}")),
            Ok(404) => {}
            Ok(c) => return State::Unknown(format!("HTTP {c}")),
            Err(err) => return State::Unknown(err),
        }
    }
    State::Free
}

/// sources.debian.org answers 200 for any name; only a non-empty version list counts.
fn debian(e: &str) -> State {
    match util::body(&format!("https://sources.debian.org/api/src/{e}/")) {
        Ok((200, b)) if b.contains("\"versions\":[{") => {
            State::Taken("Debian source package".into())
        }
        Ok((200, _)) | Ok((404, _)) => State::Free,
        Ok((c, _)) => State::Unknown(format!("HTTP {c}")),
        Err(e) => State::Unknown(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(valid_name("spawnpkg") && valid_name("a_b-c.d+e@f"));
        assert!(
            !valid_name("")
                && !valid_name("-x")
                && !valid_name("Foo")
                && !valid_name("a b")
                && !valid_name(&"a".repeat(65))
        );
    }

    #[test]
    fn json_fields() {
        let j = r#"{"crate":{"description":"An R thing","max":1,"NumVotes":42}}"#;
        assert_eq!(str_field(j, "description"), Some("An R thing"));
        assert_eq!(num_field(j, "NumVotes"), Some(42));
        assert_eq!(num_field(j, "missing"), None);
    }

    #[test]
    fn github_matching() {
        let j = r#"{"items":[{"full_name":"a/rpkg-tools","stargazers_count":9},{"full_name":"b/RPKG","stargazers_count":5},{"full_name":"c/rpkg","stargazers_count":50}]}"#;
        assert_eq!(
            github_exact(j, "rpkg"),
            State::Taken("c/rpkg (50 stars) +1 more".into())
        );
        assert_eq!(github_exact(j, "other"), State::Free);
        assert_eq!(github_exact(r#"{"items":[]}"#, "x"), State::Free);
    }

    #[test]
    fn statuses() {
        assert_eq!(from_status(404, "x"), State::Free);
        assert_eq!(from_status(200, "x"), State::Taken("x".into()));
        assert_eq!(from_status(429, "x"), State::Unknown("HTTP 429".into()));
    }
}
