#!/usr/bin/env bash
#
# dirlook release automation.
#
# Usage:
#   scripts/release.sh X.Y.Z [--dry-run] [--yes]
#
# Run from the repository root on a clean `main`.
set -euo pipefail

REPO_SLUG="magni2de/dirlook"
TAP_SLUG="magni2de/homebrew-tap"
CRATE="dirlook"
PLATFORMS="macos-arm64 macos-x86_64 linux-x86_64 linux-arm64"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION=""
DRY_RUN=0
ASSUME_YES=0

usage() {
  cat <<'EOF'
Usage: scripts/release.sh X.Y.Z [--dry-run] [--yes]

  X.Y.Z       new version, no leading 'v' (e.g. 0.4.0)
  --dry-run   print the steps without executing anything
  --yes, -y   skip the y/n confirmations
  -h, --help  show this help
EOF
}

log() { printf '\n==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

run() {
  if [ "$DRY_RUN" = 1 ]; then
    printf 'DRY-RUN: %s\n' "$*"
  else
    "$@"
  fi
}

confirm() {
  if [ "$ASSUME_YES" = 1 ] || [ "$DRY_RUN" = 1 ]; then
    return 0
  fi
  local answer
  read -r -p "$1 [y/N] " answer
  case "$answer" in
    y | Y | yes | YES) return 0 ;;
    *) return 1 ;;
  esac
}

# Print the CHANGELOG.md section for the given version (X.Y.Z), without the
# heading line. Exits non-zero when there is no section for that version.
changelog_section() {
  local file="$REPO_ROOT/CHANGELOG.md"
  [ -f "$file" ] || return 1
  awk -v hdr="## [$1]" '
    index($0, hdr) == 1 { found = 1; next }
    found && /^## / { exit }
    found { print }
  ' "$file"
}

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    --yes | -y) ASSUME_YES=1 ;;
    -h | --help) usage; exit 0 ;;
    -*) die "unknown flag '$arg'" ;;
    *)
      if [ -z "$VERSION" ]; then VERSION="$arg"; else die "unexpected argument '$arg'"; fi
      ;;
  esac
done

[ -n "$VERSION" ] || { usage; exit 2; }
printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || die "version must be X.Y.Z, got '$VERSION'"

TAG="v$VERSION"

log "Preflight"
cd "$REPO_ROOT"
[ -f Cargo.toml ] && [ -d .git ] || die "run from the repository root"
for tool in git cargo gh curl shasum; do
  command -v "$tool" >/dev/null 2>&1 || die "'$tool' not found in PATH"
done

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
[ "$BRANCH" = "main" ] || die "must be on 'main' (currently '$BRANCH')"
[ -z "$(git status --porcelain)" ] || die "working tree is dirty; commit or stash first"
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null && die "tag $TAG already exists"

OLD="$(grep -m1 '^version = ' Cargo.toml | sed -E 's/^version = "([^"]+)".*/\1/')"
[ -n "$OLD" ] || die "could not read the current version from Cargo.toml"
[ "$OLD" != "$VERSION" ] || die "version is already $VERSION"
echo "current version: $OLD -> new version: $VERSION"

log "1/7 Bump version and run checks"
run perl -i -pe "if (!\$seen && /^version = /) { s/^version = \"[^\"]*\"/version = \"$VERSION\"/; \$seen = 1 }" Cargo.toml
run cargo build
run cargo test --locked

log "2/7 Commit the bump"
run git add Cargo.toml Cargo.lock
run git commit -m "dirlook $VERSION"

log "3/7 Push main and tag"
run git push origin main
run git tag "$TAG"
run git push origin "$TAG"

log "4/7 Wait for the release assets"
if [ "$DRY_RUN" = 1 ]; then
  echo "DRY-RUN: wait until 'gh release view $TAG' lists $PLATFORMS"
else
  count=0
  for _ in $(seq 1 60); do
    got="$(gh release view "$TAG" --json assets -q '.assets | length' 2>/dev/null || echo 0)"
    if [ "${got:-0}" -ge 4 ]; then count="$got"; break; fi
    sleep 10
  done
  [ "${count:-0}" -ge 4 ] || die "release assets did not appear in time (see: gh run list)"
  echo "release assets ready: $count"
fi

log "4b/7 Attach changelog as release notes"
if notes="$(changelog_section "$VERSION")" && [ -n "$notes" ]; then
  printf '%s\n' "$notes"
  if [ "$DRY_RUN" = 1 ]; then
    echo "DRY-RUN: gh release edit $TAG --notes-file <CHANGELOG section>"
  elif confirm "Set the GitHub release notes for $TAG from CHANGELOG.md?"; then
    notes_file="$(mktemp)"
    printf '%s\n' "$notes" >"$notes_file"
    gh release edit "$TAG" --notes-file "$notes_file"
    rm -f "$notes_file"
  fi
else
  echo "warning: no CHANGELOG.md section for $VERSION; skipping release notes"
fi

log "5/7 Publish to crates.io"
if [ "$DRY_RUN" = 1 ]; then
  echo "DRY-RUN: cargo publish --dry-run && cargo publish"
else
  if curl -fsS -A "dirlook-release" -o /dev/null "https://crates.io/api/v1/crates/$CRATE/$VERSION"; then
    echo "$CRATE $VERSION is already on crates.io, skipping publish"
  elif confirm "Publish $CRATE $VERSION to crates.io?"; then
    cargo publish --dry-run
    cargo publish
  else
    die "crates.io publish declined"
  fi
fi

log "6a/7 Update README version links"
run perl -i -pe "s/\Q$OLD\E/$VERSION/g" README.md
run git add README.md
run git commit -m "docs: point README at $VERSION"
run git push origin main

log "6b/7 Bump the Homebrew formula"
if [ "$DRY_RUN" = 1 ]; then
  echo "DRY-RUN: clone $TAP_SLUG, regenerate Formula/dirlook.rb for $VERSION, commit and push"
  printf 'DRY-RUN: sha256 via curl -sL <asset-url> | shasum -a 256 for: %s\n' "$PLATFORMS"
elif confirm "Update $TAP_SLUG formula to $VERSION and push?"; then
  TMP_DIR="$(mktemp -d)"
  trap 'rm -rf "$TMP_DIR"' EXIT
  git clone -q "git@github.com:$TAP_SLUG.git" "$TMP_DIR/tap"

  base="https://github.com/$REPO_SLUG/releases/download/$TAG"
  sha_macos_arm="$(curl -sL "$base/dirlook-$TAG-macos-arm64.tar.gz" | shasum -a 256 | awk '{print $1}')"
  sha_macos_intel="$(curl -sL "$base/dirlook-$TAG-macos-x86_64.tar.gz" | shasum -a 256 | awk '{print $1}')"
  sha_linux_arm="$(curl -sL "$base/dirlook-$TAG-linux-arm64.tar.gz" | shasum -a 256 | awk '{print $1}')"
  sha_linux_intel="$(curl -sL "$base/dirlook-$TAG-linux-x86_64.tar.gz" | shasum -a 256 | awk '{print $1}')"

  cat >"$TMP_DIR/tap/Formula/dirlook.rb" <<EOF
class Dirlook < Formula
  desc "Fast, zero-dependency terminal disk usage analyzer"
  homepage "https://github.com/$REPO_SLUG"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "$base/dirlook-$TAG-macos-arm64.tar.gz"
      sha256 "$sha_macos_arm"
    end
    on_intel do
      url "$base/dirlook-$TAG-macos-x86_64.tar.gz"
      sha256 "$sha_macos_intel"
    end
  end

  on_linux do
    on_arm do
      url "$base/dirlook-$TAG-linux-arm64.tar.gz"
      sha256 "$sha_linux_arm"
    end
    on_intel do
      url "$base/dirlook-$TAG-linux-x86_64.tar.gz"
      sha256 "$sha_linux_intel"
    end
  end

  def install
    bin.install "dirlook"
  end

  test do
    assert_match "dirlook", shell_output("#{bin}/dirlook --version")
  end
end
EOF

  git -C "$TMP_DIR/tap" add Formula/dirlook.rb
  git -C "$TMP_DIR/tap" commit -m "dirlook $VERSION"
  git -C "$TMP_DIR/tap" push origin main
else
  die "Homebrew formula update declined"
fi

log "7/7 Verify"
if [ "$DRY_RUN" = 1 ]; then
  echo "DRY-RUN: curl the crates.io API, 'gh release view $TAG', 'brew upgrade dirlook'"
else
  max="$(curl -fsS -A "dirlook-release" "https://crates.io/api/v1/crates/$CRATE" | sed -E 's/.*"max_version":"([^"]+)".*/\1/')"
  echo "crates.io max_version: $max"
  gh release view "$TAG" --json tagName,assets -q '.tagName + " assets: " + (.assets | length | tostring)'
  if command -v brew >/dev/null 2>&1 && confirm "Run 'brew update && brew upgrade dirlook' now?"; then
    brew update
    brew upgrade dirlook || true
  fi
fi

log "Done: $CRATE $TAG"
