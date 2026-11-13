#!/bin/bash
# Usage: scripts/retrigger-release.sh 0.2.0
#
# Runs the release workflow for v0.2.0 again: moves the tag to the current
# commit and pushes it anew. Delete the old draft release first (gh release
# delete v0.2.0), or its assets are uploaded over.

set -euo pipefail

VERSION="${1:-}"
if [ -z "$VERSION" ]; then
  echo "usage: $0 <version>" >&2
  exit 2
fi
TAG="v$VERSION"

git push -q origin --delete "$TAG" 2>/dev/null || echo "no remote tag $TAG"
git tag -d "$TAG" 2>/dev/null || echo "no local tag $TAG"
git tag -a "$TAG" -m "Kibann Terminal $VERSION"
git push -q origin "$TAG"
echo "Pushed $TAG again."
