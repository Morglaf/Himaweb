#!/usr/bin/env python3
"""Migrate Himalaya v1-style HimaWeb import to Himalaya v2 accounts.* schema."""
from __future__ import annotations

import os
import re
from pathlib import Path


def esc(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


def main() -> None:
    appdata = os.environ.get("APPDATA") or str(Path.home() / "AppData" / "Roaming")
    path = Path(appdata) / "himalaya" / "config.toml"
    text = path.read_text(encoding="utf-8", errors="replace")
    bak = path.with_suffix(".toml.bak-himaweb")
    bak.write_text(text, encoding="utf-8")

    accounts: dict = {}
    current = None
    subsection = None  # None | backend | smtp

    for raw in text.splitlines():
        line = raw.strip()
        if not line:
            continue
        if line.startswith("#"):
            if "default-account" in line or line.startswith("default-account"):
                m = re.search(r'"([^"]+)"', line)
                if m:
                    accounts.setdefault("_meta", {})["default"] = m.group(1)
            continue
        if line.startswith("default-account"):
            m = re.search(r'"([^"]+)"', line)
            if m:
                accounts.setdefault("_meta", {})["default"] = m.group(1)
            continue
        m = re.match(r"^\[([^\]]+)\]$", line)
        if m:
            key = m.group(1)
            parts = key.split(".")
            name = parts[0]
            if name not in accounts:
                accounts[name] = {
                    "email": "",
                    "display_name": "",
                    "imap_host": "",
                    "imap_port": 993,
                    "imap_user": "",
                    "imap_pass": "",
                    "smtp_host": "",
                    "smtp_port": 465,
                    "smtp_user": "",
                    "smtp_pass": "",
                }
            if len(parts) == 1:
                current, subsection = name, None
            elif "message" in parts and "send" in parts:
                current, subsection = name, "smtp"
            elif parts[-1] == "backend":
                current, subsection = name, "backend"
            else:
                current, subsection = name, None
            continue
        if current is None or current == "_meta" or "=" not in line:
            continue
        k, v = [x.strip() for x in line.split("=", 1)]
        if v.startswith('"') and v.endswith('"'):
            v = v[1:-1]
        acc = accounts[current]
        if subsection is None:
            if k == "email":
                acc["email"] = v
            elif k == "display-name":
                acc["display_name"] = v
        elif subsection == "backend":
            if k == "host":
                acc["imap_host"] = v
            elif k == "port":
                acc["imap_port"] = int(v)
            elif k == "login":
                acc["imap_user"] = v
            elif k == "auth.cmd":
                acc["imap_pass"] = v
        elif subsection == "smtp":
            if k == "host":
                acc["smtp_host"] = v
            elif k == "port":
                acc["smtp_port"] = int(v)
            elif k == "login":
                acc["smtp_user"] = v
            elif k == "auth.cmd":
                acc["smtp_pass"] = v

    default = accounts.get("_meta", {}).get("default")
    out: list[str] = [
        "# Migré vers le format Himalaya v2 par HimaWeb",
        f"# Backup: {bak}",
        "",
    ]
    names = []
    for name, acc in accounts.items():
        if name == "_meta":
            continue
        names.append(name)
        out.append(f"[accounts.{name}]")
        if default == name:
            out.append("default = true")
        if acc["email"]:
            out.append(f'email = "{esc(acc["email"])}"')
        if acc["display_name"]:
            out.append(f'display-name = "{esc(acc["display_name"])}"')
        out.append('mailbox.alias.inbox = "Inbox"')
        out.append("")
        host = acc["imap_host"]
        port = acc["imap_port"]
        if port == 993:
            out.append(f'imap.server = "imaps://{host}:{port}"')
        else:
            out.append(f'imap.server = "imap://{host}:{port}"')
            out.append("imap.starttls = true")
        user = acc["imap_user"] or acc["email"]
        out.append(f'imap.sasl.plain.username = "{esc(user)}"')
        if acc["imap_pass"]:
            out.append(f'imap.sasl.plain.password.raw = "{esc(acc["imap_pass"])}"')
        else:
            out.append('# imap.sasl.plain.password.raw = "***"')
        out.append("")
        if acc["smtp_host"]:
            sh, sp = acc["smtp_host"], acc["smtp_port"]
            if sp == 465:
                out.append(f'smtp.server = "smtps://{sh}:{sp}"')
            else:
                out.append(f'smtp.server = "smtp://{sh}:{sp}"')
                out.append("smtp.starttls = true")
            su = acc["smtp_user"] or user
            out.append(f'smtp.sasl.plain.username = "{esc(su)}"')
            spw = acc["smtp_pass"] or acc["imap_pass"]
            if spw:
                out.append(f'smtp.sasl.plain.password.raw = "{esc(spw)}"')
            else:
                out.append('# smtp.sasl.plain.password.raw = "***"')
        out.append("")

    path.write_text("\n".join(out), encoding="utf-8")
    print(f"MIGRATED_OK count={len(names)}")
    print(f"backup={bak}")


if __name__ == "__main__":
    main()
