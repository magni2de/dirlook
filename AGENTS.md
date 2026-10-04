# AGENTS.md

Project conventions and checklists for agents working on `dirlook`.

## Release checklist

Every release must do all of the following — don't stop after publishing:

1. Bump `version` in `Cargo.toml`, update `Cargo.lock` (e.g. `cargo build`),
   and commit.
2. Push `main`, then tag `vX.Y.Z` and push the tag. This triggers
   `.github/workflows/release.yml`, which builds the four platform archives.
3. `cargo publish` (needs a crates.io token and a verified e-mail).
4. **Update the versioned links in `README.md`** — the "Prebuilt binaries"
   table must point at the new `vX.Y.Z` assets. This is easy to forget and
   leaves the README advertising the previous release.
   - macOS arm64 / x86_64, Linux x86_64 / arm64 — all four rows.
5. Bump the Homebrew formula in the separate `magni2de/homebrew-tap`
   repository (`Formula/dirlook.rb`): update all four `url` + `sha256` pairs to
   the new tag.
   - Recompute checksums with: `curl -sL <asset-url> | shasum -a 256`

## Configuration

- There is intentionally **no project-local `opencode.json`** — opencode
  settings live in the global config (`~/.config/opencode/`). Project-local
  instructions for agents belong in this file.
