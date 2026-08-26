#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "Installing main application..."
"$SCRIPT_DIR/install-main.sh"

echo "Installing providers and helper binaries..."
"$SCRIPT_DIR/install-providers.sh"

echo "All install steps completed."
