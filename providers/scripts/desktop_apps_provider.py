#!/usr/bin/env python3
import configparser
import json
import os
import re
from pathlib import Path


def desktop_dirs() -> list[Path]:
    dirs = [
        Path("/usr/share/applications"),
        Path("/usr/local/share/applications"),
    ]
    home = os.environ.get("HOME")
    if home:
        dirs.append(Path(home) / ".local/share/applications")
    return [d for d in dirs if d.exists()]


def sanitize_exec(exec_value: str) -> str:
    # Remove desktop-entry field codes like %u, %F, etc. Keep %% as literal %.
    temp = exec_value.replace("%%", "__PERCENT__")
    temp = re.sub(r"%[a-zA-Z]", "", temp)
    temp = temp.replace("__PERCENT__", "%")
    return " ".join(temp.split())


def slugify(text: str) -> str:
    slug = re.sub(r"[^a-zA-Z0-9]+", "-", text.strip().lower()).strip("-")
    return slug or "app"


def load_entry(path: Path):
    parser = configparser.ConfigParser(interpolation=None, strict=False)
    try:
        parser.read(path, encoding="utf-8")
    except Exception:
        return None

    if "Desktop Entry" not in parser:
        return None

    entry = parser["Desktop Entry"]
    if entry.get("Type", "Application") != "Application":
        return None
    if entry.get("NoDisplay", "false").lower() in {"true", "1", "yes"}:
        return None
    if entry.get("Hidden", "false").lower() in {"true", "1", "yes"}:
        return None

    name = entry.get("Name", "").strip()
    exec_value = sanitize_exec(entry.get("Exec", "").strip())
    comment = entry.get("Comment", "").strip() or "Launch desktop application"

    if not name or not exec_value:
        return None

    return {
        "name": name,
        "exec": exec_value,
        "comment": comment,
        "path": str(path),
    }


def main() -> int:
    items = []
    seen_ids: set[str] = set()

    for root in desktop_dirs():
        for desktop_file in root.rglob("*.desktop"):
            entry = load_entry(desktop_file)
            if not entry:
                continue

            item_id = slugify(entry["name"])
            if item_id in seen_ids:
                suffix = 2
                while f"{item_id}-{suffix}" in seen_ids:
                    suffix += 1
                item_id = f"{item_id}-{suffix}"
            seen_ids.add(item_id)

            items.append(
                {
                    "id": item_id,
                    "title": entry["name"],
                    "subtitle": entry["comment"],
                    "info": {
                        "summary": "Application discovered from .desktop entries.",
                        "fields": [
                            {"label": "Desktop File", "value": entry["path"]},
                            {"label": "Exec", "value": entry["exec"]},
                        ],
                    },
                    "action": {
                        "type": "shell_command_exit",
                        "value": entry["exec"],
                    },
                }
            )

    print(json.dumps(items, ensure_ascii=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
