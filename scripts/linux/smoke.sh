#!/bin/bash
# Drive a running Kibann with its CLI (run inside the Linux container,
# from the work folder, after `scripts/linux-dev.sh run`):
# split, run, read, open vim and htop. Prints what it checks.
set -euo pipefail
K=${K:-target/debug/kibann}
id() { python3 -c 'import json,sys; print(json.load(sys.stdin)["pane"]["id"])'; }
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }
fail=0

first=$($K pane list | python3 -c 'import json,sys; print(json.load(sys.stdin)["panes"][0]["id"])')
right=$($K pane split "$first" --direction right | id)
$K pane run "$right" 'printf "\e[1;31mred \e[32mgreen \e[34mblue\e[0m wide: 日本語 🎉\n"' >/dev/null
check "a split runs a command" "$K pane wait $right --match 'wide: 日本語' --timeout 5000 >/dev/null"
out=$($K pane run "$right" '(exit 3)' --wait --timeout 5000 2>&1 || true)
check "run --wait gives the exit code" '[[ "$out" == *"\"exit_code\":3"* ]]'

down=$($K pane split "$right" --direction down | id)
$K pane run "$down" 'vim -u NONE /tmp/smoke.txt' >/dev/null
check "vim opens" "$K pane wait $down --match '/tmp/smoke.txt' --timeout 5000 >/dev/null"
$K pane send "$down" 'ihello from vim' >/dev/null
$K pane keys "$down" esc >/dev/null
$K pane send "$down" ':wq' >/dev/null
$K pane keys "$down" enter >/dev/null
sleep 1
check "vim saved a file" 'grep -q "hello from vim" /tmp/smoke.txt'

tab=$($K tab new | id)
$K pane run "$tab" 'htop' >/dev/null
check "htop draws" "$K pane wait $tab --regex 'F10Quit|Quit' --timeout 5000 >/dev/null"

n=$($K pane list | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["panes"]))')
check "four panes" '[ "$n" -ge 4 ]'
exit $fail
