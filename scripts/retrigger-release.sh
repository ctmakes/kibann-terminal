#!/bin/bash
# Usage: scripts/retrigger-release.sh 0.2.0
#
# Runs the release workflow for v0.2.0 again: pushes the current commit to
# main (a fast-forward: the fix), moves the tag to it and pushes it anew.
# Only while that release is a draft: once it's published, people (and the
# updater) may have it, and a tag that moves under them is a different
# release with the same name. Release the next version instead. Delete the
# old draft first (gh release delete v0.2.0), or its assets are uploaded
# over.

set -euo pipefail

VERSION="${1:-}"
if [ -z "$VERSION" ]; then
  echo "usage: $0 <version>" >&2
  exit 2
fi
TAG="v$VERSION"

if [ "$(gh release view "$TAG" --json isDraft -q .isDraft 2>/dev/null)" = false ]; then
  echo "$TAG is published: release the next version instead (scripts/bump-version.sh)" >&2
  exit 1
fi
if [ -n "$(git status --porcelain)" ]; then
  echo "uncommitted changes: commit them first" >&2
  exit 1
fi
git push -q origin HEAD:main
git push -q origin --delete "$TAG" 2>/dev/null || echo "no remote tag $TAG"
git tag -d "$TAG" 2>/dev/null || echo "no local tag $TAG"
git tag -a "$TAG" -m "Kibann Terminal $VERSION"
git push -q origin "$TAG"
echo "Pushed $TAG again."
