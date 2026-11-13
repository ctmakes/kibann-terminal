#!/bin/bash
# Usage: scripts/bump-version.sh 0.2.0 [--local]
#
# Sets the app's version in app/package.json, app/package-lock.json,
# app/src-tauri/Cargo.toml (and Cargo.lock) and app/src-tauri/tauri.conf.json,
# dates its CHANGELOG.md section, then commits "Release 0.2.0", tags v0.2.0
# and pushes main and the tag, which starts the release workflow.
#
#   --local, -l   change the files only: no commit, tag or push

set -euo pipefail

VERSION=""
LOCAL=false
for a in "$@"; do
  case "$a" in
    --local | -l) LOCAL=true ;;
    *) VERSION="$a" ;;
  esac
done

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$ ]]; then
  echo "usage: $0 <x.y.z | x.y.z-rc.n> [--local]" >&2
  exit 2
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# A release candidate takes its notes from the version it leads to.
BASE="${VERSION%%-*}"
if ! grep -q "^## \[$BASE\]" CHANGELOG.md; then
  echo "CHANGELOG.md has no section for $BASE: write it first (## [$BASE] - Unreleased)" >&2
  exit 1
fi

if ! $LOCAL; then
  if [ -n "$(git status --porcelain)" ]; then
    echo "uncommitted changes: commit or stash them first" >&2
    exit 1
  fi
  if [ "$(git rev-parse --abbrev-ref HEAD)" != "main" ] && [[ "$VERSION" != *-rc.* ]]; then
    echo "releases are made from main" >&2
    exit 1
  fi
  if git rev-parse -q --verify "refs/tags/v$VERSION" >/dev/null; then
    echo "tag v$VERSION exists already (scripts/retrigger-release.sh reruns it)" >&2
    exit 1
  fi
fi

sedi() {
  if [[ "$OSTYPE" == darwin* ]]; then sed -i '' "$@"; else sed -i "$@"; fi
}

echo "Version $VERSION"
# The first "version" in each file is the app's.
sedi -E "1,/\"version\": \"[^\"]+\"/s/\"version\": \"[^\"]+\"/\"version\": \"$VERSION\"/" app/package.json
sedi -E "1,/\"version\": \"[^\"]+\"/s/\"version\": \"[^\"]+\"/\"version\": \"$VERSION\"/" app/src-tauri/tauri.conf.json
sedi -E "1,/^version = \"[^\"]+\"/s/^version = \"[^\"]+\"/version = \"$VERSION\"/" app/src-tauri/Cargo.toml
(cd app && npm install --package-lock-only --ignore-scripts --silent)
cargo update -p kibann --offline --quiet 2>/dev/null || cargo metadata --format-version 1 >/dev/null
if [[ "$VERSION" != *-rc.* ]]; then
  sedi -E "s/^## \[$BASE\] - Unreleased$/## [$BASE] - $(date +%Y-%m-%d)/" CHANGELOG.md
fi

git --no-pager diff --stat
$LOCAL && exit 0

git add -A app/package.json app/package-lock.json app/src-tauri/tauri.conf.json app/src-tauri/Cargo.toml Cargo.lock CHANGELOG.md
git commit -q -m "Release $VERSION"
git tag -a "v$VERSION" -m "Kibann Terminal $VERSION"
git push -q origin HEAD
git push -q origin "v$VERSION"
echo "Pushed v$VERSION: the release workflow builds a draft release (gh run watch)."
