#!/bin/sh
# Re-record the committed casts. Output varies slightly between runs.
set -e
cd "$(dirname "$0")"
rec() { cargo run -q -p term --bin vt-record -- "$@"; }
rec -o vim.cast -s 100x30 --script vim.script --cwd assets -- \
  vim -u NONE -i NONE -N -c 'syntax on' -c 'set number cursorline laststatus=2 hlsearch termguicolors' -c 'colorscheme habamax' sample.rs
rec -o man.cast -s 90x28 --script man.script -- env MANPAGER=less PAGER=less man ls
rec -o sync.cast -s 40x8 --script sync.script -- ./assets/sync.sh
