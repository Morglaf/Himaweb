# HimaWeb

Interface web locale pour l’écosystème [Pimalaya](https://pimalaya.org/) — **pas un client mail autonome**.

Principe : HimaWeb orchestre les CLI existants (Himalaya, Cardamum, Calendula, …). On n’implémente pas IMAP/SMTP/CardDAV/CalDAV soi-même : chaque action métier doit passer par l’outil Pimalaya adapté.

Stack UI : Rust / Axum, HTMX, Alpine.js, Lucide. Écoute uniquement sur `http://127.0.0.1:8787`.

## Prérequis

- Rust (édition 2021)
- [Himalaya](https://github.com/pimalaya/himalaya) configuré (`~/.config/himalaya/config.toml`)
- Optionnel : Cardamum, Calendula (et plus tard d’autres CLI Pimalaya)

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
| Mail | **Himalaya** CLI | Comptes, boîtes, listes, lecture, envoi, drapeaux, déplacement, recherche |
| Contacts | **Cardamum** CLI | Lecture / suggestions (CRUD à venir via Cardamum) |
| Calendrier | **Calendula** CLI | Lecture des événements (CRUD à venir via Calendula) |
| Config | Fichiers TOML Pimalaya | Import Thunderbird → configs Himalaya / Cardamum / Calendula ; édition comptes dans les prefs |

HimaWeb ajoute uniquement la couche web : prefs d’apparence, multi-comptes UI, HTMX, etc. — rien qui remplace Himalaya/Cardamum/Calendula.

## Architecture (pour reprendre plus tard)

```
src/
  cli/          # wrappers Himalaya / Cardamum / Calendula (JSON via --json)
  routes/       # Axum : mail, search, compose, contacts, calendar, settings
  prefs.rs      # prefs HimaWeb (UI only)
  cache/        # SQLite local (contacts / calendrier warm)
  config_form.rs, *import*, thunderbird.rs  # écriture TOML Pimalaya + import TB
templates/      # Askama + HTMX fragments
static/         # app.css / app.js (cache-bust `?v=` dans shell.html)
scripts/dev.ps1 # check binaires + cargo build + run
```

- **Bind** : `127.0.0.1:8787` uniquement (pas d’écoute réseau).
- **Appels CLI** : `CliRunner` (timeout ~60 s, sortie JSON). Concurrence limitée par un **semaphore (6)** (`AppState.cli_limit`) — à respecter sur toute fan-out (ex. recherche multi-boîtes).
- **Overrides binaires** : `HIMAWEB_HIMALAYA_BIN`, `HIMAWEB_CARDAMUM_BIN`, `HIMAWEB_CALENDULA_BIN`.
- **Configs Pimalaya** (Windows typique) : `%APPDATA%\himalaya\config.toml`, idem `cardamum` / `calendula`.
- **Données HimaWeb** : `%LOCALAPPDATA%\HimaWeb\prefs.json` + SQLite cache au même endroit. Les prefs UI (couleurs, labels, icônes, colonnes…) ne touchent pas aux configs CLI.
- **Front** : pages Askama ; interactions HTMX ; Alpine pour compose / picker icônes ; Lucide CDN. Après swap HTMX → `lucide.createIcons()`.
- **Formulaires multi-lignes** (N comptes) : `RawForm` + `form_util::parse_form_lists` — pas `Form<Vec<_>>` (serde_urlencoded casse à 1 valeur).
- **Dégradé gracieux** : sans Himalaya → page d’état ; sans Cardamum/Calendula → onglets limités, le reste tourne.

## Roadmap

### Court terme — UI

- [ ] Affiner visuellement l’ensemble de l’app (mail, recherche, compose, sidebar)
- [ ] Réorganiser et affiner l’écran Paramètres (groupes, hiérarchie, moins de densité)

### Moyen terme — données (toujours via CLI)

- [ ] Ajout / suppression / édition de contacts **via Cardamum** (éventuellement **tcard** pour l’édition vCard/TOML)
- [ ] Ajout / suppression / édition d’événements **via Calendula** (éventuellement **tcal** pour iCal/TOML)

### Écosystème Pimalaya — ponts à explorer

Objectif : maximiser l’usage de l’écosystème plutôt que de recréer des fonctions.

**Mail / sync / watch**

- [ ] **Neverest** — sync / backup mail (statut, lancer une sync depuis l’UI)
- [ ] **Mirador** — watch des boîtes (rafraîchissement live / badges sans polling maison)
- [ ] **MML** — compose / réponse en MIME Meta Language si pertinent pour l’édition riche
- [ ] **Sirup** — sessions IMAP/SMTP pré-authentifiées (perf, éviter de relancer auth à chaque commande)
- [ ] **m2m** — conversion Maildir / Maildir++ / m2dir si stockage local
- [ ] Himalaya TUI / plugins (vim, emacs, …) : pas à intégrer, mais rester compatible configs/comportements Himalaya

**Contacts & calendrier**

- [ ] **tcard** / **tcal** — édition ergonomique vCard / iCalendar (TOML) derrière les formulaires web
- [ ] Aligner CRUD contacts/événements sur les sous-commandes Cardamum / Calendula (pas d’API CardDAV/CalDAV custom)

**Sécurité & config**

- [ ] **Ortie** — OAuth 2.0 (Gmail, Google Calendar/Contacts, etc.) au lieu de bricoler les tokens
- [ ] **Pimconf** — découverte de services PIM et gestion / validation des configs depuis Paramètres

**Time**

- [ ] **Comodoro** — timers / focus liés à un mail ou un événement (optionnel)

**Bibliothèques (plus tard, si besoin)**

- [ ] Évaluer `io-email`, `io-addressbook`, `io-calendar`, `io-oauth`, `pimalaya-config`… **uniquement** si un pont CLI ne suffit plus — rester I/O-free / coroutine-friendly dans l’esprit Pimalaya, sans forker la logique métier

**Règle de design**

1. Une feature = d’abord « quelle CLI / lib Pimalaya la fait déjà ? »
2. HimaWeb = UI + orchestration + prefs locales
3. Pas de second moteur IMAP / sync / OAuth / vCard

### Plus loin — IA

- [ ] Brancher une IA (locale ou distante) pour :
  - rédaction et réponse aux mails
  - création de rendez-vous à partir d’un mail ou d’un prompt
- [ ] Préférer le local (Ollama / modèle open) avec fallback distant optionnel
- [ ] Sortie IA → actions **Himalaya / Calendula / Cardamum** (brouillon, event, contact), validation humaine avant envoi / écriture

## Licence

Usage personnel / local — à préciser si publication.
