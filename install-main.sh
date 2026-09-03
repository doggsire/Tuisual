#!/usr/bin/env bash
set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"

if [[ -n "${SUDO_USER:-}" && "${SUDO_USER}" != "root" ]]; then
  SUDO_USER_HOME="$(getent passwd "$SUDO_USER" | cut -d: -f6)"
fi

USER_HOME="${USER_HOME:-${SUDO_USER_HOME:-${HOME}}}"
HELPER_BIN_DIR="${HELPER_BIN_DIR:-${USER_HOME}/.local/bin}"
PROVIDERS_DIR="${TUISUAL_PROVIDERS_DIR:-${USER_HOME}/.config/tuisual/providers}"

NEEDS_SUDO=0
if [[ -d "$BIN_DIR" ]]; then
  if [[ ! -w "$BIN_DIR" ]]; then
    NEEDS_SUDO=1
  fi
else
  BIN_PARENT="$(dirname "$BIN_DIR")"
  if [[ ! -w "$BIN_PARENT" ]]; then
    NEEDS_SUDO=1
  fi
fi

SUDO_CMD=()
if [[ "$NEEDS_SUDO" -eq 1 ]]; then
  if ! command -v sudo >/dev/null 2>&1; then
    echo "error: sudo is required to install into $BIN_DIR but is not available" >&2
    exit 1
  fi

  echo "Requesting sudo access to install main binaries into $BIN_DIR..."
  sudo -v
  SUDO_CMD=(sudo)
fi

if [[ "$NEEDS_SUDO" -eq 1 ]]; then
  "${SUDO_CMD[@]}" mkdir -p "$BIN_DIR"
else
  mkdir -p "$BIN_DIR"
fi

cargo build --release --bin tuisual

REAL_TUISUAL_BIN="$BIN_DIR/tuisual-real"
WRAPPER_TUISUAL_BIN="$BIN_DIR/tuisual"

"${SUDO_CMD[@]}" install -m 755 "target/release/tuisual" "$REAL_TUISUAL_BIN"

WRAPPER_TMP="$(mktemp)"
cat > "$WRAPPER_TMP" <<EOF
#!/usr/bin/env bash
export TUISUAL_PROVIDERS_DIR="$PROVIDERS_DIR"
export PATH="$HELPER_BIN_DIR:\$PATH"
exec "$REAL_TUISUAL_BIN" "\$@"
EOF

"${SUDO_CMD[@]}" install -m 755 "$WRAPPER_TMP" "$WRAPPER_TUISUAL_BIN"
rm -f "$WRAPPER_TMP"

echo "Installed main app into: $WRAPPER_TUISUAL_BIN"
