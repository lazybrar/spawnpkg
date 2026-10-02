// SPDX-License-Identifier: GPL-2.0-or-later
//! `spawnpkg gate`: fmt, clippy and tests with one line of output (details only on failure).
use std::path::Path;
use std::process::{Command, Stdio};

const MAX_LINES: usize = 30;

pub fn trim_output(s: &str, max: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let mut out = lines
        .iter()
        .take(max)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    if lines.len() > max {
        out += &format!("\n... {} more lines", lines.len() - max);
    }
    out
}

pub fn count_passed(stdout: &str) -> u32 {
    stdout
        .lines()
        .filter_map(|l| l.strip_prefix("test result: ok. "))
        .filter_map(|l| l.split(" passed").next()?.trim().parse::<u32>().ok())
        .sum()
}

/// Run a cargo subcommand; Ok(stdout) or Err(trimmed diagnostics).
fn cargo(dir: &Path, args: &[&str]) -> Result<String, String> {
    let o = Command::new("cargo")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
    if o.status.success() {
        return Ok(stdout);
    }
    let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
    let text = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    Err(trim_output(&text, MAX_LINES))
}

/// Ok("fmt ok | clippy ok | tests ok (13 passed)") or Err("<step> FAILED\n<diagnostics>").
pub fn run(dir: &Path, fix: bool) -> Result<String, String> {
    let fmt_args: &[&str] = if fix { &["fmt"] } else { &["fmt", "--check"] };
    cargo(dir, fmt_args).map_err(|e| format!("fmt FAILED (run `spawnpkg gate --fix`)\n{e}"))?;
    cargo(
        dir,
        &["clippy", "--all-targets", "--quiet", "--", "-D", "warnings"],
    )
    .map_err(|e| format!("clippy FAILED\n{e}"))?;
    let out = cargo(dir, &["test", "--quiet"]).map_err(|e| format!("tests FAILED\n{e}"))?;
    Ok(format!(
        "fmt ok | clippy ok | tests ok ({} passed)",
        count_passed(&out)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_trims() {
        let out = "running 8 tests\ntest result: ok. 8 passed; 0 failed; 0 ignored\n\nrunning 5 tests\ntest result: ok. 5 passed; 0 failed\n";
        assert_eq!(count_passed(out), 13);
        assert_eq!(count_passed("nothing"), 0);
        let long = (0..50)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let t = trim_output(&long, 30);
        assert!(t.ends_with("... 20 more lines") && t.lines().count() == 31);
        assert_eq!(trim_output("a\nb", 30), "a\nb");
    }
}
