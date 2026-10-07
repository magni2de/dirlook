# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/magni2de/dirlook/compare/v0.4.2...HEAD
[0.4.2]: https://github.com/magni2de/dirlook/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/magni2de/dirlook/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/magni2de/dirlook/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/magni2de/dirlook/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/magni2de/dirlook/compare/v0.1.0...v0.2.0
