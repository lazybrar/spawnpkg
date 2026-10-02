# Changelog

## 0.3.1
- release rebuilds target/release so a PATH copy is never stale

## 0.3.0
- gate and release commands; new accepts --no-github

## 0.2.0
- `--no-github` for `check` and `new` skips the GitHub probe.

## 0.1.1
- GitHub check falls back to unauthenticated curl when `gh` is missing or not logged in (was reported as unchecked).

## 0.1.0
- `check` (9 ecosystems, `--brief`), `new`, `publish`.
