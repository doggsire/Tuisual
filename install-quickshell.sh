#!/usr/bin/env bash
set -euo pipefail

if ! command -v qs >/dev/null 2>&1; then
  echo "error: QuickShell ('qs') is required. Install the quickshell package first." >&2
  exit 1
fi

REPO_URL="${TUISUAL_REPO_URL:-https://github.com/doggsire/Tuisual.git}"

SCRIPT_DIR=""
if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi

if [[ -z "$SCRIPT_DIR" || ! -f "$SCRIPT_DIR/quickshell/shell.qml" ]]; then
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

USER_HOME="${USER_HOME:-${HOME}}"
CONFIG_DIR="${XDG_CONFIG_HOME:-$USER_HOME/.config}/quickshell/tuisual"
PREFIX="${PREFIX:-/usr/local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"

NEEDS_SUDO=0
if [[ -d "$BIN_DIR" ]]; then
  if [[ ! -w "$BIN_DIR" ]]; then
    NEEDS_SUDO=1
  fi
elif [[ ! -w "$(dirname "$BIN_DIR")" ]]; then
  NEEDS_SUDO=1
fi

SUDO_CMD=()
if [[ "$NEEDS_SUDO" -eq 1 ]]; then
  if ! command -v sudo >/dev/null 2>&1; then
    echo "error: sudo is required to install into $BIN_DIR but is not available" >&2
    exit 1
  fi
  echo "Requesting sudo access to install the launcher into $BIN_DIR..."
  sudo -v
  SUDO_CMD=(sudo)
fi

install -d "$CONFIG_DIR"
"${SUDO_CMD[@]}" install -d "$BIN_DIR"
install -m 644 "$SCRIPT_DIR/quickshell/shell.qml" "$CONFIG_DIR/shell.qml"
install -m 644 "$SCRIPT_DIR/quickshell/Theme.qml" "$CONFIG_DIR/Theme.qml"

WRAPPER_TMP="$(mktemp)"
cat > "$WRAPPER_TMP" <<EOF
#!/usr/bin/env bash
for arg in "\$@"; do
  if [[ "\$arg" == "-h" || "\$arg" == "--help" ]]; then
    echo "quisual: launch the QuickShell frontend for Tuisual."
    echo
    echo "Usage:"
    echo "  quisual                Show provider catalog"
    echo "  quisual [flags]        Load items from matching providers (forwarded to 'tuisual --json')"
    echo "  quisual -h, --help     Show this help page"
    echo
    exec tuisual -h
  fi
done
export TUISUAL_QS_ARGS="\$*"
exec qs -p "$CONFIG_DIR"
EOF
"${SUDO_CMD[@]}" install -m 755 "$WRAPPER_TMP" "$BIN_DIR/quisual"
rm -f "$WRAPPER_TMP"

echo "Installed QuickShell launcher into: $CONFIG_DIR"
echo "Installed launcher into: $BIN_DIR/quisual"
echo "Launch it with: quisual"