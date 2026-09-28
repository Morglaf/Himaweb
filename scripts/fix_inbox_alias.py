from pathlib import Path
import os

p = Path(os.environ["APPDATA"]) / "himalaya" / "config.toml"
t = p.read_text(encoding="utf-8")
t = t.replace('mailbox.alias.inbox = "INBOX"', 'mailbox.alias.inbox = "Inbox"')
p.write_text(t, encoding="utf-8")
print("ok")
