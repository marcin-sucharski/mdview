#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${MDVIEW_BIN:-"$ROOT/target/debug/mdview"}"
TMUX_BIN="${TMUX_CMD:-tmux}"
SOCKET="mdview-it-$$-$RANDOM"
SESSION="mdview"
PANE="$SESSION:0.0"
TMPDIR="$(mktemp -d)"

cleanup() {
  env -u TMUX "$TMUX_BIN" -L "$SOCKET" kill-server >/dev/null 2>&1 || true
  rm -rf "$TMPDIR"
}
trap cleanup EXIT

tmuxc() {
  env -u TMUX "$TMUX_BIN" -L "$SOCKET" "$@"
}

if [[ "${MDVIEW_SKIP_BUILD:-}" != "1" ]]; then
  (cd "$ROOT" && cargo build --quiet)
fi

capture() {
  tmuxc capture-pane -p -J -S - -E - -t "$PANE"
}

fail_with_capture() {
  local message="$1"
  printf 'integration-tmux: %s\n\n--- pane capture ---\n' "$message" >&2
  capture >&2 || true
  printf -- '--- end capture ---\n' >&2
  exit 1
}

wait_for_text() {
  local needle="$1"
  local label="${2:-$needle}"
  for _ in $(seq 1 80); do
    if capture | grep -Fq "$needle"; then
      return 0
    fi
    sleep 0.1
  done
  fail_with_capture "timed out waiting for: $label"
}

wait_until_gone() {
  local needle="$1"
  local label="${2:-$needle}"
  for _ in $(seq 1 80); do
    if ! capture | grep -Fq "$needle"; then
      return 0
    fi
    sleep 0.1
  done
  fail_with_capture "timed out waiting for text to disappear: $label"
}

start_viewer() {
  local file="$1"
  local cols="${2:-100}"
  local rows="${3:-32}"
  local extra_env="${4:-}"
  tmuxc kill-session -t "$SESSION" >/dev/null 2>&1 || true

  local command
  printf -v command 'TERM=xterm-256color %s %q %q' "$extra_env" "$BIN" "$file"
  tmuxc new-session -d -x "$cols" -y "$rows" -s "$SESSION" "$command"
}

send_key() {
  tmuxc send-keys -t "$PANE" "$@"
}

send_literal() {
  tmuxc send-keys -t "$PANE" -l "$1"
}

stop_viewer() {
  send_key q >/dev/null 2>&1 || true
  tmuxc kill-session -t "$SESSION" >/dev/null 2>&1 || true
}

printf 'integration-tmux: using %s\n' "$("$TMUX_BIN" -V)"
printf 'integration-tmux: testing %s\n' "$BIN"

start_viewer "$ROOT/examples/long.md" 100 32
wait_for_text "Long Scrolling Example" "initial long document render"
send_key j
wait_until_gone "Long Scrolling Example" "j scrolled down"
send_key k
wait_for_text "Long Scrolling Example" "k scrolled up"
send_key Down
wait_until_gone "Long Scrolling Example" "Down arrow scrolled down"
send_key Up
wait_for_text "Long Scrolling Example" "Up arrow scrolled up"
send_literal $'\033[<65;10;10M'
wait_until_gone "Long Scrolling Example" "mouse wheel scrolled down"
send_literal $'\033[<64;10;10M'
wait_for_text "Long Scrolling Example" "mouse wheel scrolled up"
send_key G
wait_for_text "End of document" "jump to bottom with G"
send_key g
wait_for_text "Long Scrolling Example" "jump to top with g"
send_key NPage
wait_until_gone "Long Scrolling Example" "PageDown moved away from top"
send_key PPage
wait_for_text "Long Scrolling Example" "PageUp returned toward top"
tmuxc resize-window -t "$SESSION" -x 58 -y 18
wait_for_text "Long Scrolling Example" "render after resize"
stop_viewer

start_viewer "$ROOT/examples/tables.md" 72 28
wait_for_text "Table Rendering" "table example render"
wait_for_text "+-" "table border render"
wait_for_text "Unicode" "table body render"
wait_for_text "wide characters" "table wrapped content render"
stop_viewer

SELECT_FILE="$TMPDIR/select.md"
printf 'alpha beta gamma\nsecond line\n' >"$SELECT_FILE"
start_viewer "$SELECT_FILE" 80 20
wait_for_text "alpha beta gamma" "selection test document"
send_literal $'\033[<0;1;1M'
send_literal $'\033[<32;6;1M'
send_literal $'\033[<0;6;1m'
wait_for_text "selected" "mouse drag selected text"
send_key y
wait_for_text "copied" "selected text copied"
stop_viewer

WATCH_FILE="$TMPDIR/watch.md"
printf '# Before\n\noriginal text\n' >"$WATCH_FILE"
start_viewer "$WATCH_FILE" 90 24
wait_for_text "Before" "initial watched file"
printf '# After normal write\n\nchanged text\n' >"$WATCH_FILE"
wait_for_text "After normal write" "reload after direct write"
TMP_WRITE="$TMPDIR/.watch.md.tmp"
printf '# After atomic rename\n\nrenamed text\n' >"$TMP_WRITE"
mv "$TMP_WRITE" "$WATCH_FILE"
wait_for_text "After atomic rename" "reload after atomic rename"
stop_viewer

start_viewer "$ROOT/examples/images.md" 180 48 "TERM_PROGRAM=tmux"
wait_for_text "Local Images" "image document through tmux"
wait_until_gone "[image: A small generated sample]" "tmux default should attempt inline image output"
wait_until_gone "not running inside iTerm2" "remote tmux should not require iTerm2 env vars"
stop_viewer

start_viewer "$ROOT/examples/images.md" 180 48 "MDVIEW_IMAGES=off TERM_PROGRAM=tmux"
wait_for_text "[image: A small generated sample]" "explicit image fallback"
wait_for_text "image output disabled by MDVIEW_IMAGES" "explicit image fallback reason"
stop_viewer

printf 'integration-tmux: ok\n'
