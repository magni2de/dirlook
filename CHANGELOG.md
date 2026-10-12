# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0] - 2026-10-12

### Added

- **Refresh the current folder.** `r` re-reads the focused folder (and its
  subfolders), so files that appear while dirlook runs — e.g. an ongoing download
  — show up without restarting.
- **Background auto-refresh (`w`, off by default).** When on, the focused folder
  is re-checked every couple of seconds and kept current. A `↻ watching` badge in
  the bottom corner shows the mode.
- **Flicker-free updates.** Refreshes are built in a detached tree and swapped in
  atomically: the tree and the map change together in a single frame, and only
  when something actually changed. The initial scan is still shown live.
- **Single help panel (`?` or `/`).** The hotkey table (left) and the color legend
  (right) now share one framed panel, separated by a thin rule.
- **Outer margins** around the tree, map, header and status, so nothing sits flush
  against the terminal edges.

### Changed

- **Clear scan highlight.** While a directory is still being scanned, its size
  cell shows a bright-yellow spinner and progress percentage, so the active part
  of the tree stands out.
- **Shorter, steady status bar.** The long key hints moved into the help panel;
  the line keeps only `size | share | sort | ?:help`, and the numbers are
  right-aligned in fixed-width fields so it no longer jitters while moving the
  cursor.
- A slow auto-refresh backs off naturally: the next check waits at least as long
  as the previous scan took.
- The release script now fails early if `AGENTS.md` would be published (tracked by
  git or included in the crate).

### Fixed

- The status bar's share figure now compares the selected item with its parent
  folder, so it is a real percentage; before, selecting any folder always showed
  a flat `100%` (it was only meaningful for files).
- Treemap blocks no longer draw their size label when it would not fit inside the
  block — it used to spill past small blocks (most visible in the stacked
  layout, near the right edge).

## [0.4.2] - 2026-10-07

### Added

- Directories that cannot be read because of missing permission are now shown
  as `denied` (with a hint in the status bar) instead of appearing empty.
- Background retry of permission-denied directories: access granted while the
  app is running is picked up without restarting.
- This changelog, and release notes pulled from it by `scripts/release.sh`.

## [0.4.1] - 2026-10-04

### Fixed

- Tree cursor no longer jumps when moving the selection up.

## [0.4.0] - 2026-10-04

### Changed

- Side-by-side layout is now the default; `m` toggles to the stacked layout.
- The tree/map divider can be moved with `[` and `]`.

## [0.3.0] - 2026-10-04

### Added

- Live, parallel background scanning with a per-directory progress bar.
- Tree branch guide lines.
- Cached treemap layout to avoid recomputing on every redraw.

## [0.2.0] - 2026-10-04

### Changed

- Tree branch lines and a clearer `Enter` hint.

## [0.1.0] - 2026-10-04

### Added

- Initial release: a zero-dependency terminal disk usage analyzer with a tree
  view and a treemap.

[Unreleased]: https://github.com/magni2de/dirlook/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/magni2de/dirlook/compare/v0.4.2...v0.5.0
[0.4.2]: https://github.com/magni2de/dirlook/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/magni2de/dirlook/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/magni2de/dirlook/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/magni2de/dirlook/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/magni2de/dirlook/compare/v0.1.0...v0.2.0
