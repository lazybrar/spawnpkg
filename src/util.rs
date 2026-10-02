// SPDX-License-Identifier: GPL-2.0-or-later
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

const UA: &str = concat!("spawnpkg/", env!("CARGO_PKG_VERSION"));

fn prog(c: &Command) -> String {
    c.get_program().to_string_lossy().into_owned()
}

/// Run a command, capture stdout; on failure return its stderr (trimmed to a few lines).
pub fn out(cmd: &mut Command) -> Result<String, String> {
    let o = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {}: {e}", prog(cmd)))?;
    if o.status.success() {
        return Ok(String::from_utf8_lossy(&o.stdout).into_owned());
    }
    let err = String::from_utf8_lossy(&o.stderr);
    let tail: Vec<&str> = err.lines().rev().take(8).collect();
    let tail: Vec<&str> = tail.into_iter().rev().collect();
    Err(format!(
        "{} failed ({}): {}",
        prog(cmd),
        o.status,
        tail.join(" | ")
    ))
}

pub fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn curl(url: &str, discard_body: bool) -> Result<(u16, String), String> {
    let mut c = Command::new("curl");
    c.args(["-sSL", "--max-time", "12", "-A", UA, "-w", "\n%{http_code}"]);
    if discard_body {
        c.args(["-o", "/dev/null"]);
    }
    let o = c
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if !o.status.success() {
        return Err(String::from_utf8_lossy(&o.stderr)
            .trim()
            .lines()
            .next()
            .unwrap_or("curl failed")
            .to_string());
    }
    let s = String::from_utf8_lossy(&o.stdout).into_owned();
    let (body, code) = s.rsplit_once('\n').unwrap_or(("", &s));
    Ok((code.trim().parse().unwrap_or(0), body.to_string()))
}

pub fn status(url: &str) -> Result<u16, String> {
    curl(url, true).map(|(c, _)| c)
}

pub fn body(url: &str) -> Result<(u16, String), String> {
    curl(url, false)
}

/// Percent-encode the few characters allowed in package names that matter in a URL path/query.
pub fn enc(name: &str) -> String {
    name.replace('@', "%40").replace('+', "%2B")
}

pub fn confirm(q: &str, yes: bool) -> bool {
    if yes {
        return true;
    }
    eprint!("{q} [y/N] ");
    let _ = io::stderr().flush();
    let mut s = String::new();
    io::stdin().lock().read_line(&mut s).is_ok()
        && matches!(s.trim().to_lowercase().as_str(), "y" | "yes")
}
