from pathlib import Path
import os

p = Path(os.environ["APPDATA"]) / "himalaya" / "config.toml"
lines = p.read_text(encoding="utf-8").splitlines()
out = []
i = 0
while i < len(lines):
    line = lines[i]
    if line.strip().startswith("# imap.sasl.plain.password.raw"):
        pw = None
        for j in range(i + 1, min(i + 12, len(lines))):
            if lines[j].startswith("[accounts."):
                break
            if "smtp.sasl.plain.password.raw" in lines[j] and not lines[j].strip().startswith("#"):
                pw = lines[j].split("=", 1)[1].strip()
                break
        if pw:
            out.append("imap.sasl.plain.password.raw = " + pw)
        else:
            out.append(line)
    else:
        out.append(line)
    i += 1
p.write_text("\n".join(out) + "\n", encoding="utf-8")
print("ok")
