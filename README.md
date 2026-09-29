# HimaWeb

Interface web locale pour l’écosystème [Pimalaya](https://pimalaya.org/) — **pas un client mail autonome**.

Principe : HimaWeb orchestre les CLI existants (Himalaya, Cardamum, Calendula, Neverest, Mirador, Ortie, …). On n’implémente pas IMAP/SMTP/CardDAV/CalDAV soi-même : chaque action métier passe par l’outil Pimalaya adapté.

Stack UI : Rust / Axum, HTMX, Alpine.js, Lucide. Écoute uniquement sur `http://127.0.0.1:8787`. Sous Windows : icône de barre système (ouvrir / redémarrer / quitter / démarrage auto).

## Prérequis

- Rust (édition 2021)
- [Himalaya](https://github.com/pimalaya/himalaya) configuré (`~/.config/himalaya/config.toml` ou `%APPDATA%\himalaya\config.toml`)
- Optionnel : Cardamum, Calendula, Neverest, Mirador, Ortie, Ollama (IA locale)

## Lancer

```powershell
.\scripts\dev.ps1
# ou
cargo run
```

Ouvrir [http://127.0.0.1:8787](http://127.0.0.1:8787).

## Ce qui est déjà branché

| Domaine | Outil | Rôle dans HimaWeb |
|--------|--------|-------------------|
| Mail | **Himalaya** CLI | Comptes, boîtes, listes, lecture, envoi, drapeaux, déplacement, suppression (alias corbeille), recherche, pièces jointes |
| Conversations | Himalaya + prefs | Groupement par fil (Message-ID / In-Reply-To / sujet) ; ouverture d’un groupe = pile de messages |
| Contacts | **Cardamum** CLI | Liste, suggestions, création / suppression |
| Calendrier | **Calendula** CLI | Lecture, création / édition / suppression d’événements |
| Sync mail | **Neverest** CLI | Lancer une sync depuis Paramètres |
| Watch | **Mirador** CLI | Watch des boîtes (complète le poll non-lus) |
| OAuth | **Ortie** CLI | Auth OAuth (ex. Gmail) depuis Paramètres |
| Config | Fichiers TOML Pimalaya | Import Thunderbird → configs Himalaya / Cardamum / Calendula ; édition comptes dans les prefs |
| IA | Ollama / Gemini | Brouillons mail & événements (validation humaine avant envoi / écriture) |
| Plugins | Dépôts git | Install / remove sous `%LOCALAPPDATA%\HimaWeb\plugins` (`himaweb-plugin.toml`) |
| Notifs | Navigateur + **NTFY** | Badges non-lus, option NTFY (serveur + topic) |
| Shell | tray-icon (Windows) | Menu ouvrir / redémarrer / quitter ; démarrage avec Windows |

HimaWeb ajoute uniquement la couche web : prefs d’apparence, multi-comptes UI, HTMX, conversations, IA, plugins — rien qui remplace les CLI Pimalaya.

## Architecture (pour reprendre plus tard)

```
src/
  cli/          # wrappers Himalaya / Cardamum / Calendula / Neverest / Mirador / Ortie
  routes/       # Axum : mail, search, compose, contacts, calendar, settings, ai
  prefs.rs      # prefs HimaWeb (UI, conversations, IA, NTFY, Mirador…)
  plugins.rs    # store local de plugins (git clone)
  tray.rs       # icône barre système Windows
  cache/        # SQLite local (messages / contacts / calendrier)
  config_form.rs, *import*, thunderbird.rs, accounts_config.rs
templates/      # Askama + HTMX fragments
static/         # app.css / app.js (cache-bust `?v=` dans shell.html)
scripts/dev.ps1 # check binaires + cargo build + run
```

- **Bind** : `127.0.0.1:8787` uniquement (pas d’écoute réseau).
- **Appels CLI** : `CliRunner` (timeout ~60 s, sortie JSON). Concurrence limitée par un **semaphore (6)** (`AppState.cli_limit`) — à respecter sur toute fan-out (ex. recherche multi-boîtes).
- **Overrides binaires** : `HIMAWEB_HIMALAYA_BIN`, `HIMAWEB_CARDAMUM_BIN`, `HIMAWEB_CALENDULA_BIN` (et chemins détectés pour Neverest / Mirador / Ortie).
- **Configs Pimalaya** (Windows typique) : `%APPDATA%\himalaya\config.toml`, idem `cardamum` / `calendula`.
- **Données HimaWeb** : `%LOCALAPPDATA%\HimaWeb\` — `prefs.json`, SQLite cache, `plugins/`. Les prefs UI ne touchent pas aux configs CLI.
- **Front** : pages Askama ; interactions HTMX ; Alpine pour compose / picker icônes ; Lucide CDN. Après swap HTMX → `lucide.createIcons()`.
- **Formulaires multi-lignes** (N comptes) : `RawForm` + `form_util::parse_form_lists` — pas `Form<Vec<_>>` (serde_urlencoded casse à 1 valeur).
- **Dégradé gracieux** : sans Himalaya → page d’état ; sans Cardamum/Calendula/Neverest/… → sections limitées, le reste tourne.

## Roadmap

### Court terme — UI

- [x] Conversations groupées + affichage du fil complet
- [x] Compose en modal, réponses / transfert, From / alias corbeille
- [x] Navigation clavier mail, suppression avec ouverture du message suivant
- [x] Tray Windows + démarrage auto
- [ ] Affiner visuellement l’ensemble de l’app (mail, recherche, compose, sidebar)
- [ ] Réorganiser et affiner l’écran Paramètres (groupes, hiérarchie, moins de densité)

### Moyen terme — données (toujours via CLI)

- [x] Création / suppression de contacts **via Cardamum**
- [ ] Édition riche de contacts (éventuellement **tcard** pour vCard/TOML)
- [x] Création / édition / suppression d’événements **via Calendula**
- [ ] Édition riche iCal (éventuellement **tcal**)

### Écosystème Pimalaya — ponts

Objectif : maximiser l’usage de l’écosystème plutôt que de recréer des fonctions.

**Mail / sync / watch**

- [x] **Neverest** — sync depuis Paramètres
- [x] **Mirador** — watch (option + poll de secours)
- [ ] **MML** — compose / réponse en MIME Meta Language si pertinent
- [ ] **Sirup** — sessions IMAP/SMTP pré-authentifiées
- [ ] **m2m** — conversion Maildir / Maildir++ / m2dir si stockage local
- [ ] Himalaya TUI / plugins (vim, emacs, …) : rester compatible configs/comportements Himalaya

**Contacts & calendrier**

- [ ] **tcard** / **tcal** — édition ergonomique vCard / iCalendar derrière les formulaires web
- [ ] Aligner le reste du CRUD sur les sous-commandes Cardamum / Calendula avancées

**Sécurité & config**

- [x] **Ortie** — OAuth depuis Paramètres
- [ ] **Pimconf** — découverte de services PIM et validation des configs

**Time**

- [ ] **Comodoro** — timers / focus liés à un mail ou un événement (optionnel)

**Bibliothèques (plus tard, si besoin)**

- [ ] Évaluer `io-email`, `io-addressbook`, `io-calendar`, `io-oauth`, `pimalaya-config`… **uniquement** si un pont CLI ne suffit plus — rester I/O-free / coroutine-friendly dans l’esprit Pimalaya, sans forker la logique métier

**Règle de design**

1. Une feature = d’abord « quelle CLI / lib Pimalaya la fait déjà ? »
2. HimaWeb = UI + orchestration + prefs locales
3. Pas de second moteur IMAP / sync / OAuth / vCard

### Plugins

- [x] Système d’install / remove depuis une URL git (`himaweb-plugin.toml`)
- [x] Prefs NTFY (serveur + topic) — plugin / notifs distantes
- [ ] Store / catalogue de plugins plus riche
- [ ] Hooks réellement invoqués par le runtime (au-delà de l’install)

### IA

- [x] Brancher Ollama (local) et Gemini (distant) pour rédaction mail / événement
- [x] Page `/ai` + APIs brouillon ; sortie validée avant envoi / écriture CLI
- [ ] Réponses contextuelles depuis un message ouvert (quote / fil)
- [ ] Création directe d’événement Calendula / contact Cardamum après validation

## Licence

Usage personnel / local — à préciser si publication.
