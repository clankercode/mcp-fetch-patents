#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET="${1:-all}"

initialize_payload='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}'

run_smoke() {
  local name="$1"
  local port="$2"
  local command="$3"
  local log="/tmp/${name}.log"

  rm -f "$log"
  (
    cd "$ROOT"
    eval "$command" >"$log" 2>&1
  ) &
  local pid=$!

  cleanup() {
    if kill -0 "$pid" >/dev/null 2>&1; then
      kill "$pid" >/dev/null 2>&1 || true
      wait "$pid" >/dev/null 2>&1 || true
    fi
  }
  trap cleanup RETURN

  for _ in $(seq 1 60); do
    if curl -fsS -X POST "http://127.0.0.1:${port}/mcp" \
      -H 'content-type: application/json' \
      -H 'accept: application/json, text/event-stream' \
      --data "$initialize_payload" >/tmp/${name}.response 2>/tmp/${name}.curl.err; then
      echo "== ${name} =="
      cat "/tmp/${name}.response"
      echo
      echo "CURL_EXIT=0"
      echo "LOGS"
      sed -n '1,120p' "$log"
      return 0
    fi
    sleep 1
  done

  echo "== ${name} =="
  echo "CURL_EXIT=7"
  echo "LOGS"
  sed -n '1,120p' "$log"
  return 1
}

case "$TARGET" in
  rust)
    run_smoke rust-http 39473 "CC=gcc cargo build --manifest-path src/rust/Cargo.toml && ./src/rust/target/debug/patent-mcp-server serve-http --host 127.0.0.1 --port 39473"
    ;;
  py-fetch)
    run_smoke py-fetch-http 39474 "uv run python -m patent_mcp serve-http --host 127.0.0.1 --port 39474"
    ;;
  py-search)
    run_smoke py-search-http 39475 "uv run python -m patent_mcp.search serve-http --host 127.0.0.1 --port 39475"
    ;;
  all)
    bash "$0" rust
    bash "$0" py-fetch
    bash "$0" py-search
    ;;
  *)
    echo "usage: $0 [rust|py-fetch|py-search|all]" >&2
    exit 2
    ;;
esac
