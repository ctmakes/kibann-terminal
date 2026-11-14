#!/bin/bash
# Build and run Kibann on Linux in a Docker container, under Xvfb.
#
#   scripts/linux-dev.sh image        build the image (once)
#   scripts/linux-dev.sh build        copy the repo in, build the app
#   scripts/linux-dev.sh test         cargo test in the container
#   scripts/linux-dev.sh start        start the container with a display
#   scripts/linux-dev.sh run          start the app on that display
#   scripts/linux-dev.sh shot OUT.png screenshot of the whole display
#   scripts/linux-dev.sh sh [CMD]     a shell (or CMD) in the container
#   scripts/linux-dev.sh stop         stop the container (volumes stay)
#
# The repo is copied (not mounted) so the Mac's node_modules and target
# stay out. Builds live in Docker volumes, so rebuilds are quick.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE=kibann-linux
NAME=kibann-linux
SIZE=${KIBANN_LINUX_SIZE:-1280x800}
ENVS=(-e DISPLAY=:99 -e XDG_RUNTIME_DIR=/tmp/run -e LANG=en_US.UTF-8 -e SHELL=/bin/zsh
  -e DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/run/bus)

running() { [ "$(docker inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null)" = true ]; }
in_box() { docker exec "${ENVS[@]}" -w /home/dev/work "$NAME" "$@"; }

start() {
  running && return
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  docker run -d --name "$NAME" --shm-size=1g \
    -v kibann-linux-work:/home/dev/work \
    -v kibann-linux-cargo:/home/dev/.cargo/registry \
    -v "$ROOT":/src:ro \
    "${ENVS[@]}" "$IMAGE" \
    bash -c "mkdir -p -m 700 /tmp/run; dbus-daemon --session --address=unix:path=/tmp/run/bus --fork; Xvfb :99 -screen 0 ${SIZE}x24 -nolisten tcp & sleep 1; openbox & sleep infinity" >/dev/null
  sleep 2
}

case "${1:-}" in
  image)
    docker build -t "$IMAGE" "$ROOT/scripts/linux"
    ;;
  start) start ;;
  build)
    start
    in_box rsync -rlt --delete --exclude node_modules --exclude target \
      --exclude 'app/dist' --exclude '.git' /src/ /home/dev/work/
    in_box bash -c 'set -o pipefail; cd app && npm ci --no-audit --no-fund >/dev/null && npx tauri build --debug --no-bundle 2>&1 | tail -20'
    ;;
  test)
    start
    in_box rsync -rlt --delete --exclude node_modules --exclude target \
      --exclude 'app/dist' --exclude '.git' /src/ /home/dev/work/
    in_box cargo test --workspace 2>&1 | grep -E "test result|FAILED|panicked|error" || true
    ;;
  run)
    start
    in_box bash -c 'pkill -x kibann || true; nohup target/debug/kibann > /tmp/app.log 2>&1 & sleep 1; echo started'
    ;;
  shot)
    out=${2:?usage: shot OUT.png}
    in_box import -window root /tmp/shot.png
    docker cp "$NAME":/tmp/shot.png "$out"
    ;;
  sh)
    shift
    start
    if [ $# -gt 0 ]; then in_box bash -lc "$*"; else docker exec -it "${ENVS[@]}" -w /home/dev/work "$NAME" zsh; fi
    ;;
  stop)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    ;;
  *)
    sed -n '2,13p' "$0"
    exit 2
    ;;
esac
