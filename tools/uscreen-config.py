#!/usr/bin/env python3
"""Read and write the top-level keys of UScreen's config.toml.

Only the flat keys above the first `[table]` section are touched: the per-tablet
profiles (`[tablets.A5EL…]`) are written by the daemon itself when a tablet
reports its panel, and a naive rewrite would either lose them or move them under
the wrong section.

    uscreen-config.py get width
    uscreen-config.py set width 2400        # int if it looks like one
    uscreen-config.py set capture wayland   # quoted string
    uscreen-config.py show
"""
import re
import sys
import tomllib
from pathlib import Path

CONFIG = Path.home() / ".config" / "uscreen" / "config.toml"


def load() -> dict:
    with CONFIG.open("rb") as f:
        return tomllib.load(f)


def render_value(text: str) -> str:
    if re.fullmatch(r"-?\d+", text):
        return text
    if text in ("true", "false"):
        return text
    return '"' + text.replace('"', '\\"') + '"'


def split_sections(lines: list[str]) -> int:
    """Index of the first line that opens a table section."""
    for i, line in enumerate(lines):
        if line.lstrip().startswith("["):
            return i
    return len(lines)


def set_key(key: str, value: str) -> bool:
    lines = CONFIG.read_text().splitlines()
    head = split_sections(lines)
    pattern = re.compile(rf"^{re.escape(key)}\s*=")
    for i in range(head):
        if pattern.match(lines[i]):
            lines[i] = f"{key} = {render_value(value)}"
            break
    else:
        lines.insert(head, f"{key} = {render_value(value)}")
    CONFIG.write_text("\n".join(lines) + "\n")
    return True


def main() -> int:
    if not CONFIG.exists():
        print(f"no config at {CONFIG}", file=sys.stderr)
        return 1
    if len(sys.argv) < 2:
        print(__doc__)
        return 1
    cmd = sys.argv[1]
    if cmd == "show":
        for k, v in load().items():
            if not isinstance(v, dict):
                print(f"{k} = {v}")
        return 0
    if cmd == "get" and len(sys.argv) == 3:
        data = load()
        if sys.argv[2] not in data:
            return 2
        print(data[sys.argv[2]])
        return 0
    if cmd == "set" and len(sys.argv) == 4:
        set_key(sys.argv[2], sys.argv[3])
        return 0
    print(__doc__)
    return 1


if __name__ == "__main__":
    sys.exit(main())
