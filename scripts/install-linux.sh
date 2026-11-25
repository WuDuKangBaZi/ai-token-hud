#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BIN_SRC="$ROOT_DIR/target/release/ai-token-hud"
BIN_DEST="${HOME}/.local/bin/ai-token-hud"
DESKTOP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
DESKTOP_FILE="$DESKTOP_DIR/ai-token-hud.desktop"

if [[ ! -x "$BIN_SRC" ]]; then
    echo "[1/3] Building release binary (ai-token-hud)..."
    (cd "$ROOT_DIR" && cargo build --release)
else
    echo "[1/3] Using existing release binary: $BIN_SRC"
fi

echo "[2/3] Installing binary to $BIN_DEST"
mkdir -p "$(dirname "$BIN_DEST")"
install -m 755 "$BIN_SRC" "$BIN_DEST"

echo "[3/3] Creating desktop entry at $DESKTOP_FILE"
mkdir -p "$DESKTOP_DIR"
cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Version=1.0
Name=AI Token HUD
Comment=Local AI Token and Cost Telemetry HUD
Exec=$BIN_DEST
Icon=utilities-system-monitor
Terminal=false
StartupNotify=false
StartupWMClass=ai-token-hud
Categories=Utility;Development;Monitor;
EOF

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DESKTOP_DIR" >/dev/null 2>&1 || true
fi

echo ""
echo "Installed successfully!"
echo "Make sure ~/.local/bin is in your PATH, then run:"
echo "  ai-token-hud"
echo ""
echo "Wayland shortcut commands (bind in GNOME/KDE Settings -> Keyboard Shortcuts):"
echo "  Show/Hide HUD:      $BIN_DEST --toggle"
echo "  Click-Through Mode: $BIN_DEST --click-through"
echo "  Pin/Collapse Panel: $BIN_DEST --flyout"
echo "  Open Studio Tab 0:  $BIN_DEST --settings 0"
