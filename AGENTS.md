# AGENTS.md

Project conventions and checklists for agents working on `dirlook`.

## Release checklist

Every release must do all of the following — don't stop after publishing.

Preconditions: feature work is committed, `CHANGELOG.md` has an entry for the
new version, and `cargo build --locked` and `cargo test --locked` pass.

1. Add a `CHANGELOG.md` section for `X.Y.Z` (Keep a Changelog format); commit
   it with the feature work.
2. Bump `version` in `Cargo.toml` to `X.Y.Z` (no leading `v`); refresh
   `Cargo.lock` (e.g. `cargo build`); commit.
3. `git push origin main`, then `git tag vX.Y.Z && git push origin vX.Y.Z`.
   This runs `.github/workflows/release.yml`, which builds the four platform
   archives.
4. Wait for the workflow to finish and the four assets to exist
   (`gh run watch` / `gh release view vX.Y.Z`), then set the release notes from
   `CHANGELOG.md`: `gh release edit vX.Y.Z --notes-file <section>`.
5. `cargo publish` (run `cargo publish --dry-run` first). Requires a clean
   git tree and a saved crates.io token (`cargo login`, once).
6. **Update the versioned links in `README.md`** — replace EVERY occurrence of
   the previous version with the new one: the "Prebuilt binaries" table
   (each row contains the version twice, in the label and the URL) **plus** the
   `tar xzf …` example command. This is easy to forget and leaves the README
   advertising the previous release. Commit & push.
7. Bump the Homebrew formula in the separate `magni2de/homebrew-tap`
   repository (`Formula/dirlook.rb`): update all four `url` + `sha256` pairs to
   the new tag.
   - Recompute checksums with: `curl -sL <asset-url> | shasum -a 256`
8. Verify: the crates.io API returns 200 with `max_version = X.Y.Z`;
   `gh release view vX.Y.Z`; `brew update && brew upgrade dirlook`.

The release script below automates the whole checklist.

## Release script

`scripts/release.sh` automates the checklist above.

```sh
scripts/release.sh 0.4.0            # interactive, asks before irreversible steps
scripts/release.sh 0.4.0 --dry-run  # print the steps, change nothing
scripts/release.sh 0.4.0 --yes      # no confirmations (unattended)
```

Prerequisites: run from the repo root on a clean `main`; `git`, `cargo`, `gh`,
`curl`, `shasum` available; a crates.io token saved with `cargo login`.

What it does: bump + commit, push `main`, tag and push, wait for the four
release assets, set the GitHub release notes from `CHANGELOG.md`, `cargo
publish`, update the README version links, regenerate the
`magni2de/homebrew-tap` formula (fresh `url` + `sha256` for all four platforms),
then verify.

What stays manual: `cargo login` (the token is never handled by the script).
The script sets the GitHub release notes from the `CHANGELOG.md` section for the
version; write that section before releasing.

## README style

- Badges go right under `# dirlook`.
- Every `##` section has an emoji icon.
- `---` dividers separate section groups.
- Language: English.
- Install order: Homebrew → Cargo → Prebuilt binaries (direct-links table) →
  From source.
- Keep every versioned URL and example in sync with the latest release.
- Never name competing apps; describe behavior in words.

## Code style

- Zero dependencies: `std` + POSIX FFI only.
- No comments unless asked.

## Configuration

- There is intentionally **no project-local `opencode.json`** — opencode
  settings live in the global config (`~/.config/opencode/`). Project-local
  instructions for agents belong in this file.
