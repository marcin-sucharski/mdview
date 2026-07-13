#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${MDVIEW_WEB_BIN:-"$ROOT/target/debug/mdview-web"}"
CURL="${CURL_BIN:-curl}"
TMPDIR="$(mktemp -d)"
WORKSPACE="$TMPDIR/workspace"
SERVER_LOG="$TMPDIR/server.log"
EVENT_LOG="$TMPDIR/events.log"
SERVER_PID=""
EVENT_PID=""

cleanup() {
  if [[ -n "$EVENT_PID" ]]; then
    kill "$EVENT_PID" >/dev/null 2>&1 || true
    wait "$EVENT_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "$SERVER_PID" ]]; then
    kill "$SERVER_PID" >/dev/null 2>&1 || true
    wait "$SERVER_PID" >/dev/null 2>&1 || true
  fi
  rm -rf "$TMPDIR"
}
trap cleanup EXIT

fail() {
  printf 'integration-web: %s\n\n--- server log ---\n' "$1" >&2
  sed -n '1,160p' "$SERVER_LOG" >&2 || true
  printf -- '--- event log ---\n' >&2
  sed -n '1,160p' "$EVENT_LOG" >&2 || true
  exit 1
}

wait_for_log() {
  local needle="$1"
  local label="$2"
  for _ in $(seq 1 100); do
    if grep -Fq "$needle" "$SERVER_LOG"; then
      return 0
    fi
    if ! kill -0 "$SERVER_PID" >/dev/null 2>&1; then
      fail "server exited while waiting for $label"
    fi
    sleep 0.05
  done
  fail "timed out waiting for $label"
}

wait_for_event() {
  local needle="$1"
  local label="$2"
  for _ in $(seq 1 100); do
    if grep -Fq "$needle" "$EVENT_LOG"; then
      return 0
    fi
    sleep 0.05
  done
  fail "timed out waiting for $label"
}

fetch_file() {
  "$CURL" -fsS --get --data-urlencode "path=$1" "$BASE/api/file"
}

wait_for_file_text() {
  local path="$1"
  local needle="$2"
  local label="$3"
  for _ in $(seq 1 100); do
    if fetch_file "$path" 2>/dev/null | grep -Fq "$needle"; then
      return 0
    fi
    sleep 0.05
  done
  fail "timed out waiting for $label"
}

if [[ "${MDVIEW_SKIP_BUILD:-}" != "1" ]]; then
  (cd "$ROOT" && cargo build --quiet --bins)
fi

mkdir -p "$WORKSPACE/docs/assets"
printf 'image bytes\n' >"$WORKSPACE/docs/assets/sample.txt"
printf '<svg xmlns="http://www.w3.org/2000/svg"><script>alert(document.origin)</script></svg>\n' \
  >"$WORKSPACE/docs/assets/active.svg"
cat >"$WORKSPACE/docs/first.md" <<'MARKDOWN'
# Web Preview

| Feature | State |
| --- | --- |
| reload | live |

```json
{"ready": true}
```

![sample](assets/sample.txt)
MARKDOWN

"$BIN" --listen 127.0.0.1 --port 0 "$WORKSPACE" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!
wait_for_log "listening on http://" "server startup"
BASE="$(sed -n 's/.*listening on \(http:\/\/[^[:space:]]*\).*/\1/p' "$SERVER_LOG" | head -1)"
[[ -n "$BASE" ]] || fail "could not determine server address"

APP_HTML="$TMPDIR/app.html"
"$CURL" -fsS "$BASE/" >"$APP_HTML"
grep -Fq 'id="tree"' "$APP_HTML" || fail "application is missing the file tree"
grep -Fq 'id="tabs"' "$APP_HTML" || fail "application is missing the tab strip"
grep -Fq 'new EventSource("/events")' "$APP_HTML" || fail "application is missing live reload"
grep -Fq 'File deleted' "$APP_HTML" || fail "application is missing its deleted-file state"
grep -Fq 'height: 100dvh' "$APP_HTML" || fail "application shell does not use the viewport height"
grep -Fq 'overflow-y: auto' "$APP_HTML" || fail "Markdown viewport is not vertically scrollable"
grep -Fq 'padding: clamp(30px, 4.5vw, 68px)' "$APP_HTML" \
  || fail "Markdown preview does not use its responsive full-width layout"
grep -Fq 'prepareMarkdownPreview' "$APP_HTML" || fail "application is missing staged preview rendering"
grep -Fq 'previewElement.replaceChildren(...staging.childNodes)' "$APP_HTML" \
  || fail "Markdown reload is not committed atomically"
grep -Fq '.tab.changed:not(.active)' "$APP_HTML" \
  || fail "application is missing the changed-tab highlight"
grep -Fq 'changed since last view' "$APP_HTML" \
  || fail "changed-tab state is not exposed accessibly"
grep -Fq 'function cancelPendingPreview()' "$APP_HTML" \
  || fail "application is missing stale-preview cancellation"
grep -Fq 'events.addEventListener("ready", () =>' "$APP_HTML" \
  || fail "application is missing SSE reconnect handling"

TREE_JSON="$TMPDIR/tree.json"
"$CURL" -fsS "$BASE/api/tree" >"$TREE_JSON"
grep -Fq 'docs/first.md' "$TREE_JSON" || fail "initial Markdown file is absent from tree"
if grep -Fq 'sample.txt' "$TREE_JSON"; then
  fail "non-Markdown file appeared in tree"
fi

FILE_JSON="$TMPDIR/file.json"
fetch_file "docs/first.md" >"$FILE_JSON"
grep -Fq '"fingerprint":"' "$FILE_JSON" || fail "Markdown response is missing its source fingerprint"
grep -Fq '\u003ctable\u003e' "$FILE_JSON" || fail "Markdown table was not rendered"
grep -Fq 'tok-key' "$FILE_JSON" || fail "JSON syntax highlighting was not rendered"
grep -Fq '/raw?path=docs/assets/sample.txt' "$FILE_JSON" || fail "local image URL was not rewritten"
"$CURL" -fsS --get --data-urlencode 'path=docs/assets/sample.txt' "$BASE/raw" | grep -Fq 'image bytes' \
  || fail "local image/resource endpoint did not return file"
SVG_HEADERS="$TMPDIR/svg.headers"
"$CURL" -fsS -D "$SVG_HEADERS" -o /dev/null --get \
  --data-urlencode 'path=docs/assets/active.svg' "$BASE/raw"
grep -Fqi 'Content-Type: image/svg+xml' "$SVG_HEADERS" \
  || fail "raw SVG response has the wrong content type"
grep -Fqi "Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; sandbox" \
  "$SVG_HEADERS" || fail "raw SVG response can execute active content"

("$CURL" -sSN --max-time 8 "$BASE/events" >"$EVENT_LOG" 2>/dev/null || true) &
EVENT_PID=$!
wait_for_event "event: ready" "SSE connection"

printf '# Direct reload\n\nchanged normally\n' >"$WORKSPACE/docs/first.md"
wait_for_event "event: change" "SSE change notification"
wait_for_file_text "docs/first.md" "Direct reload" "direct-write preview reload"

printf '# Atomic reload\n\nchanged atomically\n' >"$WORKSPACE/docs/.first.md.tmp"
mv "$WORKSPACE/docs/.first.md.tmp" "$WORKSPACE/docs/first.md"
wait_for_file_text "docs/first.md" "Atomic reload" "atomic-save preview reload"

printf '# Second file\n' >"$WORKSPACE/docs/second.md"
for _ in $(seq 1 100); do
  if "$CURL" -fsS "$BASE/api/tree" 2>/dev/null | grep -Fq 'docs/second.md'; then
    break
  fi
  sleep 0.05
done
"$CURL" -fsS "$BASE/api/tree" | grep -Fq 'docs/second.md' || fail "file tree did not refresh after creation"

rm "$WORKSPACE/docs/first.md"
STATUS="$TMPDIR/deleted.status"
for _ in $(seq 1 100); do
  code="$($CURL -sS -o "$FILE_JSON" -w '%{http_code}' --get --data-urlencode 'path=docs/first.md' "$BASE/api/file")"
  if [[ "$code" == "410" ]] && grep -Fq '"deleted":true' "$FILE_JSON"; then
    printf '%s' "$code" >"$STATUS"
    break
  fi
  sleep 0.05
done
[[ -s "$STATUS" ]] || fail "deleted open file was not reported as deleted"

printf '# Recreated file\n\nback again\n' >"$WORKSPACE/docs/first.md"
wait_for_file_text "docs/first.md" "Recreated file" "deleted-file recreation"

TRAVERSAL_CODE="$($CURL -sS -o /dev/null -w '%{http_code}' --get --data-urlencode 'path=../outside.md' "$BASE/api/file")"
[[ "$TRAVERSAL_CODE" == "400" ]] || fail "workspace path traversal was not rejected"

kill "$EVENT_PID" >/dev/null 2>&1 || true
wait "$EVENT_PID" >/dev/null 2>&1 || true
EVENT_PID=""
kill "$SERVER_PID" >/dev/null 2>&1 || true
wait "$SERVER_PID" >/dev/null 2>&1 || true
SERVER_PID=""
: >"$SERVER_LOG"
"$BIN" -I --port 0 "$WORKSPACE/docs/first.md" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!
wait_for_log "listening on http://" "single-file server startup"
grep -Eq 'listening on http://0\.0\.0\.0:[0-9]+' "$SERVER_LOG" \
  || fail "-I did not listen on all IPv4 interfaces"
BASE="$(sed -n 's/.*listening on \(http:\/\/[^[:space:]]*\).*/\1/p' "$SERVER_LOG" | head -1)"
BASE="${BASE/0.0.0.0/127.0.0.1}"
"$CURL" -fsS "$BASE/" >"$APP_HTML"
grep -Fq 'initial: "first.md"' "$APP_HTML" || fail "single-file invocation did not select its file initially"

printf 'integration-web: ok\n'
