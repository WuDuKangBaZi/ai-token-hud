#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

BIN_PATH="$SCRIPT_DIR/target/release/ai-token-hud"
if [[ -x "$BIN_PATH" ]]; then
    nohup "$BIN_PATH" "$@" >/dev/null 2>&1 &
    disown || true
else
    cargo run --release -- "$@"
fi
