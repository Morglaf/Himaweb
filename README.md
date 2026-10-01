# HimaWeb

Interface web locale pour l’écosystème [Pimalaya](https://pimalaya.org/) — **pas un client mail autonome**.

Principe : HimaWeb orchestre les CLI existants (Himalaya, Cardamum, Calendula, Neverest, Mirador, Ortie, …). On n’implémente pas IMAP/SMTP/CardDAV/CalDAV soi-même : chaque action métier passe par l’outil Pimalaya adapté.

Stack UI : Rust / Axum, Askama, HTMX, Alpine.js, Lucide. Écoute uniquement sur `http://127.0.0.1:8787`. Sous Windows : icône de barre système (ouvrir / redémarrer / quitter / démarrage auto), sans fenêtre console.

Licence : [GPL-3.0](LICENSE).

## Installation

### Binaire précompilé (recommandé)

*Unix (Linux / macOS) — root :*

```sh
curl -sSL https://raw.githubusercontent.com/Morglaf/Himaweb/master/install.sh | sudo sh
```

*Unix — utilisateur (ex. `~/.local/bin`) :*

```sh
curl -sSL https://raw.githubusercontent.com/Morglaf/Himaweb/master/install.sh | PREFIX=~/.local sh
```

*Windows (PowerShell) :*

```powershell
irm https://raw.githubusercontent.com/Morglaf/Himaweb/master/install.ps1 | iex
```

Ces commandes téléchargent la dernière [release GitHub](https://github.com/Morglaf/Himaweb/releases).

### Depuis les sources (Rust)

```sh
cargo install --locked --git https://github.com/Morglaf/Himaweb.git
```

Ou en développement :

```powershell
.\scripts\dev.ps1
# ou
cargo run
```

## Dépendances

HimaWeb ne fait rien tout seul : il appelle les binaires Pimalaya (et optionnellement Ollama) s’ils sont dans le `PATH`.

| Outil | Statut | Rôle | Installation rapide |
|-------|--------|------|---------------------|
| **[Himalaya](https://github.com/pimalaya/himalaya)** | **Obligatoire** | Mail (comptes, boîtes, lecture, envoi, recherche…) | Voir ci-dessous |
| **[Cardamum](https://github.com/pimalaya/cardamum)** | Conseillé | Contacts / suggestions d’adresses | Voir ci-dessous |
| **[Calendula](https://github.com/pimalaya/calendula)** | Conseillé | Calendrier / événements | Voir ci-dessous |
| **[Ortie](https://github.com/pimalaya/ortie)** | Conseillé | OAuth (Gmail, Microsoft, …) depuis Paramètres | Voir ci-dessous |
| **[Ollama](https://ollama.com/)** | Conseillé | IA locale (brouillons mail / événements) | Voir ci-dessous |
| **[Neverest](https://github.com/pimalaya/neverest)** | Optionnel | Sync / sauvegarde mail (depuis Paramètres) | `install.sh` Pimalaya ou `cargo install --git` |
| **[Mirador](https://github.com/pimalaya/mirador)** | Optionnel | Watch des boîtes (complète le poll non-lus) | `cargo install --git` (releases encore limitées) |
| Gemini API | Optionnel | Alternative cloud à Ollama (clé dans les prefs) | Compte Google AI |

Sans Himalaya, HimaWeb démarre mais affiche une page d’état. Sans Cardamum / Calendula / Ortie / Ollama / etc., les sections concernées sont simplement limitées.

### Installer Himalaya (obligatoire)

```sh
# Unix
curl -sSL https://raw.githubusercontent.com/pimalaya/himalaya/master/install.sh | sudo sh
# ou sans root :
curl -sSL https://raw.githubusercontent.com/pimalaya/himalaya/master/install.sh | PREFIX=~/.local sh
```

```powershell
# Windows : binaire depuis https://github.com/pimalaya/himalaya/releases
# ou Scoop : scoop install himalaya
```

Puis configurer un compte (une fois) :

```sh
himalaya
# ou
himalaya configure
```

Fichier typique : `~/.config/himalaya/config.toml` (Linux/macOS) ou `%APPDATA%\himalaya\config.toml` (Windows).

### Installer Cardamum, Calendula, Ortie (conseillés)

Même schéma que Himalaya :

```sh
curl -sSL https://raw.githubusercontent.com/pimalaya/cardamum/master/install.sh | PREFIX=~/.local sh
curl -sSL https://raw.githubusercontent.com/pimalaya/calendula/master/install.sh | PREFIX=~/.local sh
curl -sSL https://raw.githubusercontent.com/pimalaya/ortie/master/install.sh | PREFIX=~/.local sh
```

Configuration interactive :

```sh
cardamum configure   # contacts
calendula configure  # calendrier
ortie configure      # OAuth, puis : ortie auth get
```

### Installer Ollama (conseillé pour l’IA)

1. Télécharger / installer depuis [ollama.com](https://ollama.com/)
2. Lancer le service, puis tirer un modèle, par ex. :

```sh
ollama pull llama3.2
```

Dans HimaWeb → Paramètres → IA : activer Ollama (hôte local par défaut `http://127.0.0.1:11434`).

### Neverest / Mirador (optionnel)

```sh
curl -sSL https://raw.githubusercontent.com/pimalaya/neverest/master/install.sh | PREFIX=~/.local sh
# Mirador : souvent via cargo tant que les releases ne sont pas stables
cargo install --locked --git https://github.com/pimalaya/mirador.git
```

### Overrides de binaires

Si un outil n’est pas dans le `PATH` :

| Variable | Défaut |
|----------|--------|
| `HIMAWEB_HIMALAYA_BIN` | `himalaya` |
| `HIMAWEB_CARDAMUM_BIN` | `cardamum` |
| `HIMAWEB_CALENDULA_BIN` | `calendula` |
| `HIMAWEB_NEVEREST_BIN` | `neverest` |
| `HIMAWEB_MIRADOR_BIN` | `mirador` |
| `HIMAWEB_ORTIE_BIN` | `ortie` |

## Premier lancement

1. Installer **Himalaya** et avoir une config valide (`himalaya envelope list` doit fonctionner).
2. (Conseillé) Installer Cardamum, Calendula, Ortie, Ollama selon les besoins.
3. Installer HimaWeb (script ou `cargo install`).
4. Lancer :

```sh
himaweb
```

```powershell
himaweb
# Logs sous Windows :
$env:HIMAWEB_CONSOLE = "1"; himaweb
```

5. Le navigateur s’ouvre sur [http://127.0.0.1:8787](http://127.0.0.1:8787).
6. Sous Windows, une icône de barre système permet d’ouvrir / relancer / quitter / activer le démarrage auto.
7. Si aucun compte n’est encore configuré : aller dans **Paramètres** (import Thunderbird, édition des configs Pimalaya, OAuth via Ortie, sync Neverest, IA, NTFY…).

Arrêt : menu tray **Quitter**, ou `Ctrl+C` si lancé dans un terminal avec console.

## Ce qui est déjà branché

| Domaine | Outil | Rôle dans HimaWeb |
|--------|--------|-------------------|
| Mail | **Himalaya** CLI | Comptes, boîtes, listes, lecture, envoi (texte / HTML / PJ), drapeaux, déplacement (dont glisser-déposer), suppression (simple / fil / multi-sélection), recherche, téléchargement de pièces jointes |
| Conversations | Himalaya + prefs | Groupement par fil (Message-ID / In-Reply-To / sujet) ; ouverture d’un groupe = pile de messages |
| UI mail | prefs + front | Topbar (icônes / texte), rail compact, dossiers surveillés avec badge local et option « Total » (hors compteur global), multi-sélection, menu contextuel |
| Contacts | **Cardamum** CLI | Liste, suggestions, création / suppression |
| Calendrier | **Calendula** CLI | Lecture, création / édition / suppression d’événements ; widget agenda |
| Sync mail | **Neverest** CLI | Lancer une sync depuis Paramètres |
| Watch | **Mirador** CLI | Watch des boîtes (complète le poll non-lus) |
| OAuth | **Ortie** CLI | Auth OAuth (ex. Gmail) depuis Paramètres |
| Config | Fichiers TOML Pimalaya | Import Thunderbird → configs Himalaya / Cardamum / Calendula ; édition comptes dans les prefs |
| IA | Ollama / Gemini | Brouillons mail & événements (validation humaine avant envoi / écriture) |
| Plugins | Dépôts git | Install / remove sous `%LOCALAPPDATA%\HimaWeb\plugins` (`himaweb-plugin.toml`) |
| Notifs | Navigateur + **NTFY** | Badges non-lus, option NTFY (serveur + topic) |
| Shell | tray-icon (Windows) | Menu ouvrir / redémarrer / quitter ; démarrage avec Windows |

HimaWeb ajoute uniquement la couche web : prefs d’apparence, multi-comptes UI, HTMX, conversations, IA, plugins — rien qui remplace les CLI Pimalaya.

## Architecture

```
src/
  cli/          # wrappers Himalaya / Cardamum / Calendula / Neverest / Mirador / Ortie
  routes/       # Axum : mail, search, compose, contacts, calendar, settings, ai, attachments
  prefs.rs      # prefs HimaWeb (UI, dossiers surveillés, conversations, IA, NTFY, Mirador…)
  plugins.rs    # store local de plugins (git clone)
  tray.rs       # icône barre système Windows
  cache/        # SQLite local (messages / contacts / calendrier)
  config_fix.rs, *import*, thunderbird.rs, accounts_config.rs
templates/      # Askama + HTMX fragments
static/         # app.css / app.js (embarqués dans le binaire à la compilation)
scripts/dev.ps1 # check binaires + cargo build + run
install.sh      # installateur Unix (GitHub Releases)
install.ps1     # installateur Windows (GitHub Releases)
```

- **Bind** : `127.0.0.1:8787` uniquement (pas d’écoute réseau).
- **Appels CLI** : `CliRunner` (timeout ~60 s, sortie JSON, `CREATE_NO_WINDOW` sous Windows). Concurrence limitée par un **semaphore (6)** (`AppState.cli_limit`).
- **Configs Pimalaya** (Windows typique) : `%APPDATA%\himalaya\config.toml`, idem `cardamum` / `calendula` / `ortie`.
- **Données HimaWeb** : `%LOCALAPPDATA%\HimaWeb\` — `prefs.json`, SQLite cache, `plugins/`. Les prefs UI ne touchent pas aux configs CLI.
- **Front** : pages Askama ; interactions HTMX ; Alpine pour compose / picker icônes ; Lucide CDN. Après swap HTMX → `lucide.createIcons()`.
- **Formulaires multi-lignes** (N comptes) : `RawForm` + `form_util::parse_form_lists` — pas `Form<Vec<_>>` (serde_urlencoded casse à 1 valeur).
- **Compose** : `multipart/form-data` pour les pièces jointes ; EML multipart (texte / HTML / PJ) avec en-tête `Date` RFC 2822.
- **Dégradé gracieux** : sans Himalaya → page d’état ; sans Cardamum/Calendula/Neverest/… → sections limitées, le reste tourne.

## Publier une release

Après de grosses modifs (sur `master`, avec `gh` connecté) :

```powershell
# Tout committer d’abord, puis bump patch (0.1.0 → 0.1.1) + tag + push :
.\scripts\release.ps1

# Ou inclure les fichiers encore non commités dans le commit de release :
.\scripts\release.ps1 -IncludeChanges

# Minor / major / version exacte :
.\scripts\release.ps1 -Bump minor
.\scripts\release.ps1 -Version 0.2.0

# Attendre la fin de la CI :
.\scripts\release.ps1 -Wait

# Simulation :
.\scripts\release.ps1 -DryRun
```

Équivalent manuel :

```sh
# 1. bump version dans Cargo.toml, commit
git tag v0.1.1
git push origin master
git push origin v0.1.1
```

Le workflow [`.github/workflows/release.yml`](.github/workflows/release.yml) construit les binaires (Linux x64/arm64, macOS x64/arm64, Windows x64) et les attache à la release GitHub. `install.sh` / `install.ps1` pointent vers `…/releases/latest/download/…` — **attendez que la CI soit verte** avant de réinstaller.

## Roadmap

### Moyen terme — données (toujours via CLI)

- [ ] Édition riche de contacts (éventuellement **tcard** pour vCard/TOML)
- [ ] Édition riche iCal (éventuellement **tcal**)

### Écosystème Pimalaya — ponts

Objectif : maximiser l’usage de l’écosystème plutôt que de recréer des fonctions.

**Mail / sync / watch**

- [ ] **MML** — compose / réponse en MIME Meta Language si pertinent
- [ ] **Sirup** — sessions IMAP/SMTP pré-authentifiées
- [ ] **m2m** — conversion Maildir / Maildir++ / m2dir si stockage local
- [ ] Himalaya TUI / plugins (vim, emacs, …) : rester compatible configs/comportements Himalaya

**Contacts & calendrier**

- [ ] **tcard** / **tcal** — édition ergonomique vCard / iCalendar derrière les formulaires web
- [ ] Aligner le reste du CRUD sur les sous-commandes Cardamum / Calendula avancées

**Sécurité & config**
- [ ] **Pimconf** — découverte de services PIM et validation des configs

**Time**

- [ ] **Comodoro** — timers / focus liés à un mail ou un événement (optionnel)

**Bibliothèques (plus tard, si besoin)**

- [ ] Évaluer `io-email`, `io-addressbook`, `io-calendar`, `io-oauth`, `pimalaya-config`… **uniquement** si un pont CLI ne suffit plus — rester I/O-free / coroutine-friendly dans l’esprit Pimalaya, sans forker la logique métier

**Règle de design**

1. Une feature = d’abord « quelle CLI / lib Pimalaya la fait déjà ? »
2. HimaWeb = UI + orchestration + prefs locales
3. Pas de second moteur IMAP / sync / OAuth / vCard

## Licence

[GNU General Public License v3.0](LICENSE) (GPL-3.0).
