#!/usr/bin/env bash
set -euo pipefail

# ─── StainedSteel Uninstaller ─────────────────────────────────────────

BIN_NAME="stained-steel"
BIN_DIR="$HOME/.local/bin"
CONFIG_DIR="$HOME/.config/stained-steel"
SERVICE_DIR="$HOME/.config/systemd/user"
UDEV_RULES="/etc/udev/rules.d/99-steelseries.rules"

echo "╔══════════════════════════════════════╗"
echo "║  StainedSteel — Uninstall            ║"
echo "╚══════════════════════════════════════╝"
echo

# ─── 1. Stop and disable service ──────────────────────────────────────
if systemctl --user is-active --quiet "$BIN_NAME.service" 2>/dev/null; then
    echo "▸ Stopping service..."
    systemctl --user stop "$BIN_NAME.service"
fi
if systemctl --user is-enabled --quiet "$BIN_NAME.service" 2>/dev/null; then
    echo "▸ Disabling service..."
    systemctl --user disable "$BIN_NAME.service"
fi

# ─── 2. Remove service file ───────────────────────────────────────────
if [ -f "$SERVICE_DIR/$BIN_NAME.service" ]; then
    echo "▸ Removing service file"
    rm -f "$SERVICE_DIR/$BIN_NAME.service"
    systemctl --user daemon-reload
fi

# ─── 3. Remove binary ─────────────────────────────────────────────────
if [ -f "$BIN_DIR/$BIN_NAME" ]; then
    echo "▸ Removing binary"
    rm -f "$BIN_DIR/$BIN_NAME"
fi

# ─── 4. Config ────────────────────────────────────────────────────────
if [ -d "$CONFIG_DIR" ]; then
    read -rp "▸ Remove config directory $CONFIG_DIR? [y/N] " answer
    if [[ "$answer" =~ ^[Yy]$ ]]; then
        rm -rf "$CONFIG_DIR"
        echo "  ✓ Config removed"
    else
        echo "  → Config kept"
    fi
fi

# ─── 5. udev rules ────────────────────────────────────────────────────
if [ -f "$UDEV_RULES" ]; then
    read -rp "▸ Remove udev rules? (requires sudo) [y/N] " answer
    if [[ "$answer" =~ ^[Yy]$ ]]; then
        sudo rm -f "$UDEV_RULES"
        sudo udevadm control --reload-rules
        echo "  ✓ udev rules removed"
    else
        echo "  → udev rules kept"
    fi
fi

echo
echo "  StainedSteel has been uninstalled."
