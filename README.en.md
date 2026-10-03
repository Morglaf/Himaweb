# HimaWeb

[Français](README.md) · [English](README.en.md)

Local web UI for the [Pimalaya](https://pimalaya.org/) ecosystem — **not a standalone mail client**.

Idea: HimaWeb orchestrates existing CLIs (Himalaya, Cardamum, Calendula, Neverest, Mirador, Ortie, …). We do not implement IMAP/SMTP/CardDAV/CalDAV ourselves: every domain action goes through the matching Pimalaya tool.

UI stack: Rust / Axum, Askama, HTMX, Alpine.js, Lucide. Listens only on `http://127.0.0.1:8787`. On Windows: system tray icon (open / restart / quit / start with Windows), no console window.

License: [GPL-3.0](LICENSE).

## Acknowledgments

Huge thanks to the **[Pimalaya](https://pimalaya.org/)** team and community for all their work — the CLIs, libraries, documentation, and the vision of a free, modular, I/O-free PIM ecosystem. HimaWeb would not exist without Himalaya, Cardamum, Calendula, Neverest, Mirador, Ortie, and the rest of the Pimalaya family. Thank you for this solid foundation and for the energy poured into open source.

## Installation

### Prebuilt binary (recommended)

*Unix (Linux / macOS) — root:*

```sh
curl -sSL https://raw.githubusercontent.com/Morglaf/Himaweb/master/install.sh | sudo sh
```

*Unix — user install (e.g. `~/.local/bin`):*

```sh
curl -sSL https://raw.githubusercontent.com/Morglaf/Himaweb/master/install.sh | PREFIX=~/.local sh
```

*Windows — GUI installer (recommended):*

Download **[HimaWeb-Setup-x64.exe](https://github.com/Morglaf/Himaweb/releases/latest/download/HimaWeb-Setup-x64.exe)** (Inno Setup): HimaWeb plus optional Pimalaya dependencies (Himalaya, Cardamum, Calendula, Ortie, …).

Or via **Winget** / **UniGet** (automatic updates once the package is published):

```powershell
winget install Morglaf.HimaWeb
winget upgrade Morglaf.HimaWeb
```

*Windows — silent script (portable / CI):*

```powershell
irm https://raw.githubusercontent.com/Morglaf/Himaweb/master/install.ps1 | iex
```

These commands target the latest [GitHub release](https://github.com/Morglaf/Himaweb/releases).

### From source (Rust)

```sh
cargo install --locked --git https://github.com/Morglaf/Himaweb.git
```

Or for development:

```powershell
.\scripts\dev.ps1
# or
cargo run
```

## Dependencies

HimaWeb does nothing on its own: it calls Pimalaya binaries (and optionally Ollama) if they are on your `PATH`.

| Tool                                                   | Status          | Role                                               | Quick install                                    |
| ------------------------------------------------------| ----------------| ----------------------------------------------------| --------------------------------------------------|
| **[Himalaya](https://github.com/pimalaya/himalaya)**   | **Required**    | Mail (accounts, folders, read, send, search…)      | See below                                        |
| **[Cardamum](https://github.com/pimalaya/cardamum)**   | Recommended     | Contacts / address suggestions                     | See below                                        |
| **[Calendula](https://github.com/pimalaya/calendula)** | Recommended     | Calendar / events                                  | See below                                        |
| **[Ortie](https://github.com/pimalaya/ortie)**         | Recommended     | OAuth (Gmail, Microsoft, …) from Settings          | See below                                        |
| **[Ollama](https://ollama.com/)**                      | Recommended     | Local AI (mail / event drafts)                     | See below                                        |
| **[Neverest](https://github.com/pimalaya/neverest)**   | Optional        | Mail sync / backup (from Settings)                 | Pimalaya `install.sh` or `cargo install --git`   |
| **[Mirador](https://github.com/pimalaya/mirador)**     | Optional        | Folder watch (complements unread polling)          | `cargo install --git` (releases still limited)   |
| Gemini API                                             | Optional        | Cloud alternative to Ollama (key in prefs)         | Google AI account                                |

Without Himalaya, HimaWeb starts but shows a status page. Without Cardamum / Calendula / Ortie / Ollama / etc., the related sections are simply limited.

### Install Himalaya (required)

```sh
# Unix
curl -sSL https://raw.githubusercontent.com/pimalaya/himalaya/master/install.sh | sudo sh
# or without root:
curl -sSL https://raw.githubusercontent.com/pimalaya/himalaya/master/install.sh | PREFIX=~/.local sh
```

```powershell
# Windows: binary from https://github.com/pimalaya/himalaya/releases
# or Scoop: scoop install himalaya
```

Then configure an account (once):

```sh
himalaya
# or
himalaya configure
```

Typical config path: `~/.config/himalaya/config.toml` (Linux/macOS) or `%APPDATA%\himalaya\config.toml` (Windows).

### Install Cardamum, Calendula, Ortie (recommended)

Same pattern as Himalaya:

```sh
curl -sSL https://raw.githubusercontent.com/pimalaya/cardamum/master/install.sh | PREFIX=~/.local sh
curl -sSL https://raw.githubusercontent.com/pimalaya/calendula/master/install.sh | PREFIX=~/.local sh
curl -sSL https://raw.githubusercontent.com/pimalaya/ortie/master/install.sh | PREFIX=~/.local sh
```

Interactive setup:

```sh
cardamum configure   # contacts
calendula configure  # calendar
ortie configure      # OAuth, then: ortie auth get
```

### Install Ollama (recommended for AI)

1. Download / install from [ollama.com](https://ollama.com/)
2. Start the service, then pull a model, e.g.:

```sh
ollama pull llama3.2
```

In HimaWeb → Settings → AI: enable Ollama (default local host `http://127.0.0.1:11434`).

### Neverest / Mirador (optional)

```sh
curl -sSL https://raw.githubusercontent.com/pimalaya/neverest/master/install.sh | PREFIX=~/.local sh
# Mirador: often via cargo until releases stabilize
cargo install --locked --git https://github.com/pimalaya/mirador.git
```

### Binary overrides

If a tool is not on your `PATH`:

| Variable | Default |
|----------|--------|
| `HIMAWEB_HIMALAYA_BIN` | `himalaya` |
| `HIMAWEB_CARDAMUM_BIN` | `cardamum` |
| `HIMAWEB_CALENDULA_BIN` | `calendula` |
| `HIMAWEB_NEVEREST_BIN` | `neverest` |
| `HIMAWEB_MIRADOR_BIN` | `mirador` |
| `HIMAWEB_ORTIE_BIN` | `ortie` |

## First launch

1. Install **Himalaya** and have a valid config (`himalaya envelope list` should work).
2. (Recommended) Install Cardamum, Calendula, Ortie, Ollama as needed.
3. Install HimaWeb (script or `cargo install`).
4. Run:

```sh
himaweb
```

```powershell
himaweb
# Logs on Windows:
$env:HIMAWEB_CONSOLE = "1"; himaweb
```

5. The browser opens at [http://127.0.0.1:8787](http://127.0.0.1:8787).
6. On Windows, a tray icon lets you open / restart / quit / enable start with Windows.
7. If no account is configured yet: go to **Settings** (Thunderbird import, Pimalaya config editing, OAuth via Ortie, Neverest sync, AI, NTFY…).

Stop: tray menu **Quit**, or `Ctrl+C` if started in a terminal with console.

## What is already wired

| Domain | Tool | Role in HimaWeb |
|--------|------|-----------------|
| Mail | **Himalaya** CLI | Accounts, folders, lists, read, send (text / HTML / attachments), flags, move (incl. drag-and-drop), delete (single / thread / multi-select), search, attachment download |
| Conversations | Himalaya + prefs | Thread grouping (Message-ID / In-Reply-To / subject); opening a group = message stack |
| Mail UI | prefs + front | Topbar (icons / text), compact rail, watched folders with local badge and optional “Total” (outside global counter), multi-select, context menu |
| Contacts | **Cardamum** + **tcard** | List, suggestions, create / edit / delete, photos (vCard ↔ TOML) |
| Calendar | **Calendula** + **tcal** | Read, create / edit / delete events and todos (iCal ↔ TOML); agenda widget |
| Mail sync | **Neverest** CLI | Run a sync from Settings |
| Watch | **Mirador** CLI | Folder watch (complements unread polling) |
| OAuth | **Ortie** CLI | OAuth auth (e.g. Gmail) from Settings |
| Config | Pimalaya TOML files | Thunderbird import → Himalaya / Cardamum / Calendula configs; account editing in prefs |
| AI | Ollama / Gemini | Mail & event drafts (human validation before send / write) |
| Plugins | Git repos | Install / remove under `%LOCALAPPDATA%\HimaWeb\plugins` (`himaweb-plugin.toml`) |
| Notifs | Browser + **NTFY** | Unread badges, optional NTFY (server + topic) |
| Shell | tray-icon (Windows) | Open / restart / quit menu; start with Windows |

HimaWeb only adds the web layer: appearance prefs, multi-account UI, HTMX, conversations, AI, plugins — nothing that replaces the Pimalaya CLIs.

## Architecture

```
src/
  cli/          # Himalaya / Cardamum / Calendula / Neverest / Mirador / Ortie wrappers + tcard / tcal bridges
  routes/       # Axum: mail, search, compose, contacts, calendar, settings, ai, attachments
  prefs.rs      # HimaWeb prefs (UI, watched folders, conversations, AI, NTFY, Mirador…)
  plugins.rs    # local plugin store (git clone)
  tray.rs       # Windows system tray icon
  cache/        # local SQLite (messages / contacts / calendar)
  config_fix.rs, *import*, thunderbird.rs, accounts_config.rs
templates/      # Askama + HTMX fragments
static/         # app.css / app.js (embedded in the binary at build time)
scripts/dev.ps1 # check binaries + cargo build + run
scripts/build-installer.ps1  # build HimaWeb-Setup-x64.exe (Inno Setup 6)
installer/      # himaweb.iss + install-deps.ps1 (Pimalaya deps in parallel)
install.sh      # Unix installer (GitHub Releases)
install.ps1     # silent Windows installer (fallback)
```

- **Bind**: `127.0.0.1:8787` only (no network listen).
- **CLI calls**: `CliRunner` (~60 s timeout, JSON output, `CREATE_NO_WINDOW` on Windows). Concurrency limited by a **semaphore (6)** (`AppState.cli_limit`).
- **Pimalaya configs** (typical Windows): `%APPDATA%\himalaya\config.toml`, same for `cardamum` / `calendula` / `ortie`.
- **HimaWeb data**: `%LOCALAPPDATA%\HimaWeb\` — `prefs.json`, SQLite cache, `plugins/`. UI prefs do not touch CLI configs.
- **Front**: Askama pages; HTMX interactions; Alpine for compose / icon picker; Lucide CDN. After HTMX swap → `lucide.createIcons()`.
- **Multi-row forms** (N accounts): `RawForm` + `form_util::parse_form_lists` — not `Form<Vec<_>>` (serde_urlencoded breaks at 1 value).
- **Compose**: `multipart/form-data` for attachments; multipart EML (text / HTML / attachments) with RFC 2822 `Date` header.
- **Graceful degradation**: without Himalaya → status page; without Cardamum/Calendula/Neverest/… → limited sections, the rest still runs.

## Publishing a release

After substantial changes (on `master`, with `gh` authenticated):

```powershell
# Commit everything first, then bump patch (0.1.0 → 0.1.1) + tag + push:
.\scripts\release.ps1

# Or include still-uncommitted files in the release commit:
.\scripts\release.ps1 -IncludeChanges

# Minor / major / exact version:
.\scripts\release.ps1 -Bump minor
.\scripts\release.ps1 -Version 0.2.0

# Wait for CI to finish:
.\scripts\release.ps1 -Wait

# Dry run:
.\scripts\release.ps1 -DryRun
```

Manual equivalent:

```sh
# 1. bump version in Cargo.toml, commit
git tag v0.1.1
git push origin master
git push origin v0.1.1
```

The [`.github/workflows/release.yml`](.github/workflows/release.yml) workflow builds binaries (Linux x64/arm64, macOS x64/arm64, Windows x64) and attaches them to the GitHub release. `install.sh` / `install.ps1` point at `…/releases/latest/download/…` — **wait for CI to be green** before reinstalling.

## Roadmap

### Done
- [x] **tcard** — rich vCard editing behind contact forms
- [x] **tcal** — rich iCalendar editing behind calendar forms (events + todos)

### Todo
- [x] UI translations: **Spanish**, **German**, **Italian** (in addition to FR / EN)
- [ ] **Pimconf** — PIM service discovery and config validation
- [ ] **Comodoro** — timers / focus tied to a mail or event (optional)

**Design rule**

1. Maximize use of the ecosystem rather than reimplementing features.
2. A feature = first ask “which Pimalaya CLI / lib already does this?”
3. HimaWeb = UI + orchestration + local prefs
4. No second IMAP / sync / OAuth / vCard / iCalendar engine
5. Himalaya TUI / plugins (vim, emacs, …): stay compatible with Himalaya configs/behavior

## License

[GNU General Public License v3.0](LICENSE) (GPL-3.0).
