# spawnpkg

Three steps of starting a small Rust tool, as one command each: check the name everywhere, scaffold the project,
publish it to GitHub. Zero dependencies, written in Rust. Uses `curl`, `git`, `cargo`, and (optionally) `gh`,
`pacman` and [cratepkg](https://github.com/lazybrar/cratepkg).

## Check a name

    spawnpkg check foo bar            # verdict + only the clashes
    spawnpkg check foo --all          # every source
    spawnpkg check foo bar --brief    # one line per name, minimal output (for scripts and AI)
    spawnpkg check foo --no-github    # skip the GitHub probe (also accepted by `new`)

Sources (probed in parallel, about a second in total):

| blocking (`!`) | informational |
|---|---|
| Arch official repos (local sync db, resolves `provides`), AUR, crates.io, your `$PATH` | GitHub repos with exactly that name, npm, PyPI, Homebrew, Debian |

Brief format: `foo taken:!crates,github ?:npm` means taken on crates.io (blocking) and GitHub, npm could not be checked.
Exit status: 0 free everywhere, 1 taken somewhere, 2 usage error.

## Scaffold

    spawnpkg new foo --desc "What it does"

Checks the name first (refuses on a blocking clash unless `--allow-taken`), then creates `~/work/projects/small/foo`
(`$SPAWNPKG_BASE` or `--dir` to change): `Cargo.toml`, `src/main.rs`, GPL-2.0-or-later `LICENSE`, README, CHANGELOG,
CI workflow, `Cargo.lock`, `pkg/PKGBUILD` (if `cratepkg` is installed) and a first commit (`--trailer "Co-Authored-By: ..."`).

## Publish

    spawnpkg publish ~/work/projects/small/foo --public     # or --private

Requires a clean git tree and a LICENSE, runs `cargo fmt --check` and `cargo test`, asks for confirmation (`--yes` skips),
then creates the repo under your logged-in `gh` account, pushes, and adds Cargo `keywords` as topics.
Visibility is never defaulted.

## Gate

    spawnpkg gate            # fmt --check, clippy -D warnings, tests
    spawnpkg gate --fix      # run `cargo fmt` instead of checking

Success is one line: `fmt ok | clippy ok | tests ok (22 passed)`. On failure it stops at the first failing step and prints
that step's diagnostics, trimmed to 30 lines. Exit status 0 or 1.

## Release

    spawnpkg release patch -m "what changed"            # or minor / major
    spawnpkg release minor -m "..." --push --trailer "Co-Authored-By: ..."

Runs the gate, bumps the version in `Cargo.toml`, adds a `CHANGELOG.md` entry, refreshes `Cargo.lock`, rebuilds `target/release`, regenerates
`pkg/PKGBUILD` and rebuilds the package (needs `cratepkg`; keeps a custom `--pkgname` / `--no-check`), commits
`<name> <ver>: <message>`, tags `v<ver>`, and with `--push` pushes the branch and tag. Uncommitted changes are included in
the release commit. `--dry-run` shows the plan; `--no-package` and `--skip-gate` skip those steps. Output is 2-3 lines.

## Install

    cd pkg && makepkg -si

## License

GPL-2.0-or-later. See `LICENSE`.
