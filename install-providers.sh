#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${SUDO_USER:-}" && "${SUDO_USER}" != "root" ]]; then
  SUDO_USER_HOME="$(getent passwd "$SUDO_USER" | cut -d: -f6)"
fi

USER_HOME="${USER_HOME:-${SUDO_USER_HOME:-${HOME}}}"
HELPER_BIN_DIR="${HELPER_BIN_DIR:-${USER_HOME}/.local/bin}"
PROVIDERS_DIR="${TUISUAL_PROVIDERS_DIR:-${USER_HOME}/.config/tuisual/providers}"

mkdir -p "$HELPER_BIN_DIR" "$PROVIDERS_DIR"

cargo build --release --bin desktop_apps_provider --bin path_commands_provider --bin pkg_manager_provider

install -m 755 "target/release/desktop_apps_provider" "$HELPER_BIN_DIR/desktop_apps_provider"
install -m 755 "target/release/path_commands_provider" "$HELPER_BIN_DIR/path_commands_provider"
install -m 755 "target/release/pkg_manager_provider" "$HELPER_BIN_DIR/pkg_manager_provider"

for file in providers/*.json; do
  [ -e "$file" ] || continue
    if [[ "$(basename "$file")" == "example.json" ]]; then
        continue
    fi
  cp "$file" "$PROVIDERS_DIR/"
done

rm -f "$PROVIDERS_DIR/example.json"

python3 - "$PROVIDERS_DIR" "$HELPER_BIN_DIR" "providers" <<'PY'
import json
import sys
from pathlib import Path

providers_dir = Path(sys.argv[1])
bin_dir = Path(sys.argv[2])
source_dir = Path(sys.argv[3])

managed_provider_bins = {
  "desktop_apps_provider",
  "path_commands_provider",
  "pkg_manager_provider",
}

source_filenames = {
  path.name
  for path in source_dir.glob("*.json")
  if path.name != "example.json"
}

source_provider_name_to_file = {}
for source_path in source_dir.glob("*.json"):
  if source_path.name == "example.json":
    continue
  try:
    source_data = json.loads(source_path.read_text())
  except json.JSONDecodeError:
    continue
  if not isinstance(source_data, dict):
    continue
  source_name = source_data.get("name")
  if isinstance(source_name, str) and source_name.strip():
    source_provider_name_to_file[source_name.strip()] = source_path.name

for provider_path in providers_dir.glob('*.json'):
  if provider_path.name in {"example.json"}:
    provider_path.unlink(missing_ok=True)
    continue

  if provider_path.name in source_filenames:
    continue

  try:
    data = json.loads(provider_path.read_text())
  except json.JSONDecodeError:
    continue

  if not isinstance(data, dict):
    continue

  command = data.get('command')
  if not isinstance(command, str):
    continue

  if Path(command).name in managed_provider_bins:
    provider_path.unlink(missing_ok=True)

for provider_path in providers_dir.glob('*.json'):
    try:
        data = json.loads(provider_path.read_text())
    except json.JSONDecodeError:
        continue

    if not isinstance(data, dict):
        continue

    provider_name = data.get("name")
    if isinstance(provider_name, str):
      expected_file = source_provider_name_to_file.get(provider_name.strip())
      if expected_file and expected_file != provider_path.name:
        provider_path.unlink(missing_ok=True)
        continue

    command = data.get('command')
    if not isinstance(command, str):
        continue

    if command.startswith('./target/') or command.startswith('target/'):
        binary_name = Path(command).name
        data['command'] = str(bin_dir / binary_name)
        provider_path.write_text(json.dumps(data, indent=2) + '\n')
PY

echo "Installed provider definitions into: $PROVIDERS_DIR"
echo "Installed helper binaries into: $HELPER_BIN_DIR"
