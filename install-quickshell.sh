#!/usr/bin/env bash
set -euo pipefail

if ! command -v qs >/dev/null 2>&1; then
  echo "error: QuickShell ('qs') is required. Install the quickshell package first." >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
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
    echo "tuisual-qs: launch the QuickShell frontend for Tuisual."
    echo
    echo "Usage:"
    echo "  tuisual-qs                Show provider catalog"
    echo "  tuisual-qs [flags]        Load items from matching providers (forwarded to 'tuisual --json')"
    echo "  tuisual-qs -h, --help     Show this help page"
    echo
    exec tuisual -h
  fi
done
export TUISUAL_QS_ARGS="\$*"
exec qs -p "$CONFIG_DIR"
EOF
"${SUDO_CMD[@]}" install -m 755 "$WRAPPER_TMP" "$BIN_DIR/tuisual-qs"
rm -f "$WRAPPER_TMP"

echo "Installed QuickShell launcher into: $CONFIG_DIR"
echo "Installed launcher into: $BIN_DIR/tuisual-qs"
echo "Launch it with: tuisual-qs"