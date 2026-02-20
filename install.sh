#!/usr/bin/env bash
set -euo pipefail

# ─── StainedSteel Installer ───────────────────────────────────────────
# Builds the release binary, installs it, sets up config, udev rules,
# and a systemd user service for auto-start on login.
# ──────────────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_NAME="stained-steel"
BIN_DIR="$HOME/.local/bin"
CONFIG_DIR="$HOME/.config/stained-steel"
SERVICE_DIR="$HOME/.config/systemd/user"
UDEV_RULES="/etc/udev/rules.d/99-steelseries.rules"

echo "╔══════════════════════════════════════╗"
echo "║   StainedSteel — OLED Driver Setup   ║"
echo "╚══════════════════════════════════════╝"
echo

# ─── 1. Build ─────────────────────────────────────────────────────────
echo "▸ Building release binary..."
cd "$SCRIPT_DIR"
cargo build --release --locked
echo "  ✓ Build complete"
echo

# ─── 2. Install binary ────────────────────────────────────────────────
echo "▸ Installing binary to $BIN_DIR/$BIN_NAME"
mkdir -p "$BIN_DIR"
cp -f "$SCRIPT_DIR/target/release/stained_steel_rust" "$BIN_DIR/$BIN_NAME"
chmod +x "$BIN_DIR/$BIN_NAME"
if command -v strip >/dev/null 2>&1; then
    strip "$BIN_DIR/$BIN_NAME" || true
    echo "  ✓ Binary stripped"
else
    echo "  → strip not found, skipping"
fi
echo "  ✓ Binary installed"
echo

# ─── 3. Install config ────────────────────────────────────────────────
echo "▸ Installing config to $CONFIG_DIR/"
mkdir -p "$CONFIG_DIR"
for cfg in "$SCRIPT_DIR"/profiles/*.json; do
    cp -f "$cfg" "$CONFIG_DIR/"
done
echo "  ✓ Config files overwritten from profiles/*.json"
echo

# ─── 4. udev rules (needs sudo) ───────────────────────────────────────
echo "▸ Installing udev rules for SteelSeries HID access"
if [ -f "$UDEV_RULES" ]; then
    echo "  → udev rules already exist at $UDEV_RULES"
else
    echo "  → This requires sudo to write to /etc/udev/rules.d/"
    sudo cp -f "$SCRIPT_DIR/99-steelseries.rules" "$UDEV_RULES"
    sudo udevadm control --reload-rules
    sudo udevadm trigger
    echo "  ✓ udev rules installed and reloaded"
fi
echo

# ─── 5. systemd user service ──────────────────────────────────────────
echo "▸ Installing systemd user service"
mkdir -p "$SERVICE_DIR"
cp -f "$SCRIPT_DIR/stained-steel.service" "$SERVICE_DIR/$BIN_NAME.service"
systemctl --user daemon-reload
systemctl --user enable "$BIN_NAME.service"
echo "  ✓ Service enabled (will auto-start on login)"
echo

# ─── 6. Start now ─────────────────────────────────────────────────────
echo "▸ Starting StainedSteel..."
systemctl --user restart "$BIN_NAME.service"
sleep 1
if systemctl --user is-active --quiet "$BIN_NAME.service"; then
    echo "  ✓ Running!"
else
    echo "  ✗ Service failed to start. Check logs:"
    echo "    journalctl --user -u $BIN_NAME.service -n 20"
fi
echo

# ─── Done ──────────────────────────────────────────────────────────────
echo "════════════════════════════════════════"
echo "  Installation complete!"
echo ""
echo "  Binary:   $BIN_DIR/$BIN_NAME"
echo "  Config:   $CONFIG_DIR/dashboard.json"
echo "  Service:  $BIN_NAME.service (systemd user)"
echo ""
echo "  Useful commands:"
echo "    systemctl --user status $BIN_NAME"
echo "    systemctl --user stop $BIN_NAME"
echo "    systemctl --user restart $BIN_NAME"
echo "    journalctl --user -u $BIN_NAME -f"
echo ""
echo "  Make sure ~/.local/bin is in your PATH."
echo "════════════════════════════════════════"
