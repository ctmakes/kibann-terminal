#!/bin/sh
# Refresh the bundled color themes from iTerm2-Color-Schemes (MIT), its
# Ghostty-format folder, into one file the app embeds:
#   app/src-tauri/themes/themes.txt   "### NAME" then the theme's lines
#   app/src-tauri/themes/LICENSE      the collection's license
# Usage: scripts/update-themes.sh [COMMIT]   (default: the one below)
set -eu
COMMIT=${1:-e5964e1410f2f5092e7a0d66be91055bf9e78b14}
REPO=https://github.com/mbadolato/iTerm2-Color-Schemes.git
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$ROOT/app/src-tauri/themes
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

git -C "$TMP" init -q
git -C "$TMP" remote add origin "$REPO"
git -C "$TMP" sparse-checkout set ghostty
git -C "$TMP" fetch -q --depth 1 --filter=blob:none origin "$COMMIT"
git -C "$TMP" checkout -q FETCH_HEAD

mkdir -p "$OUT"
cp "$TMP/LICENSE" "$OUT/LICENSE"
{
  echo "# iTerm2-Color-Schemes $COMMIT, ghostty/ (MIT, see LICENSE)"
  for f in "$TMP"/ghostty/*; do
    [ -f "$f" ] || continue
    printf '### %s\n' "$(basename "$f")"
    # Theme lines only: no blank lines, CR stripped.
    tr -d '\r' < "$f" | grep -v '^[[:space:]]*$' || true
  done
} > "$OUT/themes.txt"
echo "$(grep -c '^### ' "$OUT/themes.txt") themes, $(wc -c < "$OUT/themes.txt") bytes"
