#!/usr/bin/env bash
set -euo pipefail

REPO_URL="${TUISUAL_REPO_URL:-https://github.com/doggsire/Tuisual.git}"

SCRIPT_DIR=""
if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi

if [[ -z "$SCRIPT_DIR" || ! -f "$SCRIPT_DIR/Cargo.toml" ]]; then
  # Running via `curl | bash`, no local checkout to reference — fetch one.
  if ! command -v git >/dev/null 2>&1; then
    echo "error: git is required to install without a local checkout" >&2
    exit 1
  fi
  TMP_DIR="$(mktemp -d)"
  trap 'rm -rf "$TMP_DIR"' EXIT
  echo "Cloning Tuisual repository..."
  git clone --depth 1 "$REPO_URL" "$TMP_DIR/tuisual"
  SCRIPT_DIR="$TMP_DIR/tuisual"
fi

cd "$SCRIPT_DIR"

echo "Installing main application..."
"$SCRIPT_DIR/install-main.sh"

echo "Installing providers and helper binaries..."
"$SCRIPT_DIR/install-providers.sh"

echo "All install steps completed."
