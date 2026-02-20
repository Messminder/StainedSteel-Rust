#!/usr/bin/env bash
set -euo pipefail

SERVICE_NAME="stained-steel.service"

echo "▸ Restarting ${SERVICE_NAME}..."
systemctl --user restart "${SERVICE_NAME}"

echo "▸ Current status:"
systemctl --user status "${SERVICE_NAME}" --no-pager -n 20
