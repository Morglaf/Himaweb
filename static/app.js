/* Assets chargés à la demande ------------------------------------------- */

/** Version des assets embarqués, lue sur l'URL de ce script. */
function hwAssetVersion() {
  if (hwAssetVersion._v !== undefined) return hwAssetVersion._v;
  let v = '';
  try {
    const src = (document.currentScript && document.currentScript.src) || '';
    v = new URL(src, location.href).searchParams.get('v') || '';
  } catch (_) {}
  if (!v) {
    const el = document.querySelector('script[src*="/static/app.js"]');
    if (el) {
      try {
        v = new URL(el.src, location.href).searchParams.get('v') || '';
      } catch (_) {}
    }
  }
  hwAssetVersion._v = v;
  return v;
}

const hwPendingAssets = {};

function hwLoadAsset(url, kind) {
  if (hwPendingAssets[url]) return hwPendingAssets[url];
  hwPendingAssets[url] = new Promise((resolve, reject) => {
    const el =
      kind === 'css'
        ? Object.assign(document.createElement('link'), { rel: 'stylesheet', href: url })
        : Object.assign(document.createElement('script'), { src: url });
    el.addEventListener('load', () => resolve());
    el.addEventListener('error', () => reject(new Error(`asset: ${url}`)));
    document.head.appendChild(el);
  });
  return hwPendingAssets[url];
}

/**
 * Quill ne sert qu'à la rédaction HTML : 230 Ko hors du chemin critique,
 * chargés à la première ouverture d'un éditeur.
 */
function ensureQuill() {
  if (typeof Quill !== 'undefined') return Promise.resolve();
  const v = hwAssetVersion();
  const q = v ? `?v=${encodeURIComponent(v)}` : '';
  return Promise.all([
    hwLoadAsset(`/static/vendor/quill.snow.css${q}`, 'css'),
    hwLoadAsset(`/static/vendor/quill.js${q}`, 'js'),
  ]);
}

/* Icônes Lucide : rendu scopé et idempotent ------------------------------ */

/**
 * `lucide.createIcons()` rescanne tout le document ET reconstruit les icônes
 * déjà rendues (le `<svg>` produit conserve `data-lucide`). Sur une liste de
 * 50 messages — plus de 300 icônes — chaque appel recrée donc tout le DOM des
 * icônes, plusieurs fois par action à cause des appels empilés.
 *
 * On marque les icônes traitées et on sait se limiter à un sous-arbre, en
 * réutilisant `lucide.icons` et `lucide.createElement` (API publiques).
 */
const HW_ICON_DONE = 'data-hw-icon';

function hwIconPascal(name) {
  return name.replace(/(\w)(\w*)(_|-|\s*)/g, (_m, head, tail) => head.toUpperCase() + tail.toLowerCase());
}

function hwRenderIcons(root) {
  const lu = window.lucide;
  if (!lu || !lu.icons || typeof lu.createElement !== 'function') return 0;

  const scope = root && typeof root.querySelectorAll === 'function' ? root : document;
  const sel = `[data-lucide]:not([${HW_ICON_DONE}])`;
  const targets = [];
  if (scope.matches && scope.matches(sel)) targets.push(scope);
  scope.querySelectorAll(sel).forEach((el) => targets.push(el));

  let done = 0;
  targets.forEach((el) => {
    const name = el.getAttribute('data-lucide');
    if (!name) return;
    const node = lu.icons[hwIconPascal(name)];
    if (!node) return;
    const [tag, baseAttrs, children] = node;
    const attrs = { ...baseAttrs };
    for (const a of el.attributes) attrs[a.name] = a.value;
    attrs['data-lucide'] = name;
    attrs[HW_ICON_DONE] = '1';
    attrs.class = `lucide lucide-${name} ${el.getAttribute('class') || ''}`
      .trim()
      .split(/\s+/)
      .filter((c, i, all) => c && all.indexOf(c) === i)
      .join(' ');
    const svg = lu.createElement([tag, attrs, children]);
    if (el.parentNode) {
      el.parentNode.replaceChild(svg, el);
      done += 1;
    }
  });
  return done;
}

/**
 * Les appels `lucide.createIcons()` sont dispersés dans les templates et les
 * expressions Alpine. Plutôt que de les réécrire un par un, on redirige
 * l'entrée publique vers la version scopée ; la signature d'origine reste
 * disponible pour un appel explicitement paramétré.
 */
(function hwInstallIconShim() {
  const lu = window.lucide;
  if (!lu || lu.__hwScoped) return;
  if (!lu.icons || typeof lu.createElement !== 'function') return;
  const original = lu.createIcons;
  lu.__hwScoped = true;
  lu.createIcons = function hwCreateIcons(arg) {
    if (arg && arg.nodeType === 1) return hwRenderIcons(arg);
    const configured = arg && typeof arg === 'object' && (arg.icons || arg.nameAttr || arg.attrs);
    if (configured) return original.call(lu, arg);
    return hwRenderIcons(document);
  };
})();

/* Barre de progression globale ------------------------------------------ */

/**
 * Indicateur unique pour toute requête en vol, HTMX comme `fetch`.
 *
 * Un délai de grâce évite le clignotement sur les réponses immédiates
 * (fragments servis depuis le cache), et la progression tend vers 92 % sans
 * jamais l'atteindre : on ne connaît pas la durée réelle d'un appel CLI.
 */
const HWProgress = (() => {
  const GRACE_MS = 120;
  const TICK_MS = 180;
  const CEILING = 92;

  let el = null;
  let active = 0;
  let pct = 0;
  let graceTimer = null;
  let tickTimer = null;
  let hideTimer = null;

  function bar() {
    if (el && el.isConnected) return el;
    el = document.createElement('div');
    el.className = 'hw-progress';
    el.setAttribute('aria-hidden', 'true');
    document.body.appendChild(el);
    return el;
  }

  function paint() {
    bar().style.width = `${pct}%`;
  }

  function show() {
    pct = 8;
    bar().classList.add('is-active');
    paint();
    clearInterval(tickTimer);
    tickTimer = setInterval(() => {
      pct += (CEILING - pct) * 0.14;
      paint();
    }, TICK_MS);
  }

  function finish() {
    clearTimeout(graceTimer);
    graceTimer = null;
    clearInterval(tickTimer);
    tickTimer = null;
    // Jamais affichée : la requête a répondu dans le délai de grâce.
    if (!el || !el.classList.contains('is-active')) {
      if (el) el.style.width = '0';
      pct = 0;
      return;
    }
    pct = 100;
    paint();
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      if (active > 0) return;
      const b = bar();
      b.classList.remove('is-active');
      setTimeout(() => {
        if (active === 0) {
          pct = 0;
          b.style.width = '0';
        }
      }, 280);
    }, 180);
  }

  return {
    begin() {
      active += 1;
      if (active !== 1) return;
      clearTimeout(hideTimer);
      hideTimer = null;
      clearTimeout(graceTimer);
      graceTimer = setTimeout(() => {
        graceTimer = null;
        if (active > 0) show();
      }, GRACE_MS);
    },
    end() {
      active = Math.max(0, active - 1);
      if (active === 0) finish();
    },
    /** Encadre une promesse par begin/end, quoi qu'il arrive. */
    async track(promise) {
      this.begin();
      try {
        return await promise;
      } finally {
        this.end();
      }
    },
  };
})();

window.HWProgress = HWProgress;

/* Requêtes HTMX en vol : un seul compteur, dédupliqué par XHR. */
const hwTrackedXhr = new WeakMap();

document.addEventListener('htmx:beforeRequest', (ev) => {
  const xhr = ev.detail && ev.detail.xhr;
  if (xhr) {
    if (hwTrackedXhr.has(xhr)) return;
    hwTrackedXhr.set(xhr, true);
  }
  HWProgress.begin();
});

function hwRequestSettled(ev) {
  const xhr = ev.detail && ev.detail.xhr;
  if (xhr) {
    if (!hwTrackedXhr.has(xhr)) return;
    hwTrackedXhr.delete(xhr);
  }
  HWProgress.end();
}

['htmx:afterRequest', 'htmx:sendError', 'htmx:timeout', 'htmx:abort'].forEach((name) =>
  document.addEventListener(name, hwRequestSettled),
);

/* Filet de sécurité : une requête en échec ne produit aucun swap, les zones
   marquées occupées resteraient figées sur leur squelette. */
['htmx:responseError', 'htmx:sendError', 'htmx:timeout', 'htmx:abort'].forEach((name) =>
  document.addEventListener(name, () => {
    ['message-pane', 'envelope-list'].forEach((id) => {
      const el = document.getElementById(id);
      if (!el || !el.hasAttribute('aria-busy')) return;
      el.removeAttribute('aria-busy');
      if (el.querySelector('.hw-skeleton')) {
        el.innerHTML =
          '<div class="empty-list"><p class="muted">Échec du chargement</p></div>';
      }
    });
    document
      .querySelectorAll('.envelope.is-opening, .envelope.is-pending')
      .forEach((el) => el.classList.remove('is-opening', 'is-pending'));
  }),
);

function contactsPage() {
  return {
    openCreate: false,
    selected: null,
    selectFromEl(el) {
      if (!el || !el.dataset) return;
      this.selected = {
        id: el.dataset.id || '',
        name: el.dataset.name || '',
        email: el.dataset.email || '',
        book: el.dataset.book || '',
        book_ref: el.dataset.bookRef || '',
        initial: el.dataset.initial || '?',
      };
      this.$nextTick(() => { if (window.lucide) lucide.createIcons(); });
    },
    isSelected(el) {
      if (!this.selected || !el || !el.dataset) return false;
      return (
        this.selected.email === (el.dataset.email || '') &&
        this.selected.id === (el.dataset.id || '')
      );
    },
  };
}

function aiSettings(opts) {
  opts = opts || {};
  return {
    provider: opts.provider || 'ollama',
    model: opts.model || 'gemini-2.5-flash',
    models: opts.models || [],
    scanning: false,
    scanError: '',
    async scanModels() {
      this.scanning = true;
      this.scanError = '';
      try {
        const typed = (this.$refs.apiKey && this.$refs.apiKey.value) || '';
        const res = await fetch('/settings/ai/models', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ api_key: typed }),
        });
        const data = await res.json();
        if (data.error) throw new Error(data.error);
        this.models = data.models || [];
        if (this.models.length && !this.models.includes(this.model)) {
          this.model = this.models[0];
        }
        if (!this.models.length) this.scanError = 'Aucun modèle generateContent trouvé.';
      } catch (e) {
        this.scanError = e.message || String(e);
      } finally {
        this.scanning = false;
      }
    },
  };
}

function composeForm() {
  return {
    cardamum: false,
    ai: false,
    aiOpen: false,
    kind: 'compose',
    showCc: false,
    showBcc: false,
    showReplyTo: false,
    toolbarMode: 'icon-text',
    to: '',
    cc: '',
    bcc: '',
    replyTo: '',
    subject: '',
    body: '',
    bodyHtml: '',
    bodyMode: 'plain',
    account: '',
    accounts: [],
    fromOpen: false,
    aiPrompt: '',
    aiBusy: false,
    aiError: '',
    draftBusy: false,
    attachNames: [],
    attachFiles: [],
    windowMode: 'normal',
    baseTitle: 'Nouveau message',
    quill: null,
    suggestions: { to: [], cc: [], bcc: [] },
    get selectedAccount() {
      return this.accounts.find((a) => a.name === this.account) || this.accounts[0] || null;
    },
    get displayTitle() {
      const s = (this.subject || '').trim();
      return s || this.baseTitle;
    },
    boot() {
      try {
        const saved = sessionStorage.getItem('himaweb-compose-window');
        if (saved === 'docked' || saved === 'minimized' || saved === 'normal') {
          this.windowMode = saved;
        }
      } catch (_) {}
      try {
        const mode = sessionStorage.getItem('himaweb-compose-body-mode');
        if (mode === 'html' || mode === 'plain') this.bodyMode = mode;
      } catch (_) {}
      try {
        const el = document.getElementById('compose-boot');
        if (el) {
          const opts = JSON.parse(el.textContent || '{}');
          this.cardamum = !!opts.cardamum;
          this.ai = !!opts.ai;
          this.kind = opts.kind || 'compose';
          this.baseTitle = opts.title || 'Nouveau message';
          this.toolbarMode =
            opts.toolbarMode === 'icon' || opts.toolbarMode === 'text' || opts.toolbarMode === 'icon-text'
              ? opts.toolbarMode
              : 'icon-text';
          this.to = opts.to || '';
          this.cc = opts.cc || '';
          this.bcc = opts.bcc || '';
          this.replyTo = opts.replyTo || '';
          this.subject = opts.subject || '';
          this.accounts = opts.accounts || [];
          this.account = opts.account || (this.accounts[0] && this.accounts[0].name) || '';
          this.showCc = !!this.cc;
          this.showBcc = !!this.bcc;
          this.showReplyTo = !!this.replyTo;
        }
      } catch (_) {}
      const seed = document.getElementById('compose-body-seed');
      if (seed) this.body = seed.value || '';
      this.$nextTick(() => {
        if (window.lucide) lucide.createIcons();
        if (this.bodyMode === 'html') this.initQuill();
      });
    },
    pickAccount(a) {
      if (!a) return;
      this.account = a.name;
      this.fromOpen = false;
      this.$nextTick(() => {
        if (window.lucide) lucide.createIcons();
      });
    },
    setBodyMode(mode) {
      if (mode === this.bodyMode) return;
      if (mode === 'html') {
        this.syncHtmlFromPlain();
        this.bodyMode = 'html';
        this.$nextTick(() => {
          this.destroyQuill();
          this.initQuill();
          if (window.lucide) lucide.createIcons();
        });
      } else {
        this.syncPlainFromQuill();
        this.bodyMode = 'plain';
        this.destroyQuill();
        this.$nextTick(() => {
          if (window.lucide) lucide.createIcons();
        });
      }
      try {
        sessionStorage.setItem('himaweb-compose-body-mode', this.bodyMode);
      } catch (_) {}
    },
    currentBodyText() {
      if (this.bodyMode === 'html') {
        if (this.quill) {
          return (this.quill.getText() || '').replace(/\n$/, '').trim();
        }
        const tmp = document.createElement('div');
        tmp.innerHTML = this.bodyHtml || '';
        return ((tmp.innerText || tmp.textContent || '') + '').trim();
      }
      return (this.body || '').trim();
    },
    draftContext() {
      const lines = [];
      if (this.to) lines.push('À: ' + this.to);
      if (this.cc) lines.push('Cc: ' + this.cc);
      if (this.bcc) lines.push('Cci: ' + this.bcc);
      if (this.replyTo) lines.push('Reply-To: ' + this.replyTo);
      if (this.subject) lines.push('Sujet: ' + this.subject);
      const { draftBody, previous } = this.splitDraftAndPrevious();
      if (draftBody) lines.push('Corps:\n' + draftBody);
      return {
        context: lines.join('\n\n') || '(brouillon vide)',
        previous: previous || '',
      };
    },
    splitDraftAndPrevious() {
      const full = this.currentBodyText();
      if (!full) return { draftBody: '', previous: '' };
      // Sépare le brouillon utilisateur du message cité (reply)
      const markers = [
        /\nOn .+ wrote:\n/,
        /\nLe .+ a écrit\s*:\n/i,
        /\n-{2,}\s*Original Message\s*-{2,}\n/i,
        /\n_{2,}\nFrom:\s/i,
        /\n> /,
      ];
      let cut = -1;
      for (const re of markers) {
        const m = full.search(re);
        if (m >= 0 && (cut < 0 || m < cut)) cut = m;
      }
      if (cut > 0) {
        return {
          draftBody: full.slice(0, cut).trim(),
          previous: full.slice(cut).trim(),
        };
      }
      if (this.kind === 'reply' && full.includes('\n>')) {
        const idx = full.indexOf('\n>');
        if (idx > 0) {
          return {
            draftBody: full.slice(0, idx).trim(),
            previous: full.slice(idx).trim(),
          };
        }
      }
      return { draftBody: full, previous: '' };
    },
    localNowLabel() {
      try {
        const d = new Date();
        const pad = (n) => String(n).padStart(2, '0');
        const tz = Intl.DateTimeFormat().resolvedOptions().timeZone || '';
        return (
          d.getFullYear() +
          '-' +
          pad(d.getMonth() + 1) +
          '-' +
          pad(d.getDate()) +
          ' ' +
          pad(d.getHours()) +
          ':' +
          pad(d.getMinutes()) +
          (tz ? ' (' + tz + ')' : '')
        );
      } catch (_) {
        return new Date().toISOString();
      }
    },
    syncHtmlFromPlain() {
      const plain = this.body || '';
      if (!plain) {
        this.bodyHtml = '';
        return;
      }
      this.bodyHtml = plain
        .split('\n')
        .map((line) => {
          const esc = line
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;');
          return '<p>' + (esc || '<br>') + '</p>';
        })
        .join('');
    },
    syncPlainFromQuill() {
      if (this.quill) {
        this.bodyHtml = this.quill.root.innerHTML || '';
        this.body = this.quill.getText().replace(/\n$/, '');
      } else if (this.bodyHtml) {
        const tmp = document.createElement('div');
        tmp.innerHTML = this.bodyHtml;
        this.body = (tmp.innerText || tmp.textContent || '').replace(/\n$/, '');
      }
    },
    initQuill() {
      if (this.quill || this._quillIniting) return;
      this._quillIniting = true;
      const tryInit = (attempt) => {
        if (typeof Quill === 'undefined') {
          if (attempt < 40) {
            setTimeout(() => tryInit(attempt + 1), 50);
          } else {
            this._quillIniting = false;
          }
          return;
        }
        const wrap = document.querySelector('.compose-quill-wrap');
        if (!wrap) {
          this._quillIniting = false;
          return;
        }
        // Remet une coque propre (évite toolbars empilées)
        wrap.innerHTML = '<div id="compose-quill" class="compose-quill"></div>';
        const host = document.getElementById('compose-quill');
        if (!host) {
          this._quillIniting = false;
          return;
        }
        this.quill = new Quill(host, {
          theme: 'snow',
          modules: {
            toolbar: [
              ['bold', 'italic', 'underline'],
              [{ list: 'ordered' }, { list: 'bullet' }],
              ['link'],
              ['clean'],
            ],
          },
        });
        if (!this.bodyHtml && this.body) this.syncHtmlFromPlain();
        if (this.bodyHtml) {
          this.quill.clipboard.dangerouslyPasteHTML(this.bodyHtml);
        }
        this.quill.on('text-change', () => {
          this.bodyHtml = this.quill.root.innerHTML || '';
          this.body = this.quill.getText().replace(/\n$/, '');
        });
        this._quillIniting = false;
      };
      ensureQuill().then(
        () => tryInit(0),
        () => {
          this._quillIniting = false;
        },
      );
    },
    destroyQuill() {
      this._quillIniting = false;
      this.quill = null;
      const wrap = document.querySelector('.compose-quill-wrap');
      if (wrap) {
        wrap.innerHTML = '<div id="compose-quill" class="compose-quill"></div>';
      }
    },
    onSubmit(ev) {
      const submitter = ev.submitter;
      this.draftBusy = !!(submitter && submitter.name === 'save_draft');
      if (this.bodyMode === 'html' && this.quill) {
        this.bodyHtml = this.quill.root.innerHTML || '';
        this.body = this.quill.getText().replace(/\n$/, '');
      }
    },
    onFilesChange(ev) {
      const incoming = [...((ev.target && ev.target.files) || [])];
      for (const f of incoming) {
        const key = `${f.name}\0${f.size}\0${f.lastModified}`;
        const exists = this.attachFiles.some(
          (x) => `${x.name}\0${x.size}\0${x.lastModified}` === key
        );
        if (!exists) this.attachFiles.push(f);
      }
      this.syncAttachInput();
    },
    removeAttach(idx) {
      if (idx < 0 || idx >= this.attachFiles.length) return;
      this.attachFiles.splice(idx, 1);
      this.syncAttachInput();
    },
    syncAttachInput() {
      this.attachNames = this.attachFiles.map((f) => f.name);
      const input = this.$refs.attachInput;
      if (input) {
        const dt = new DataTransfer();
        this.attachFiles.forEach((f) => dt.items.add(f));
        input.files = dt.files;
      }
      this.$nextTick(() => {
        if (window.lucide) lucide.createIcons();
      });
    },
    setWindow(mode) {
      this.windowMode = mode;
      try {
        sessionStorage.setItem('himaweb-compose-window', mode);
      } catch (_) {}
      this.$nextTick(() => {
        if (window.lucide) lucide.createIcons();
        if (this.bodyMode === 'html' && !this.quill) this.initQuill();
      });
    },
    toggleMinimize() {
      this.setWindow(this.windowMode === 'minimized' ? 'docked' : 'minimized');
    },
    onEscape() {
      if (this.fromOpen) {
        this.fromOpen = false;
        return;
      }
      if (this.windowMode === 'normal') this.goBack();
      else this.setWindow('normal');
    },
    goBack() {
      if (window.HimaWeb && window.HimaWeb.closeComposeOverlay()) return;
      if (window.history.length > 1) window.history.back();
      else window.location.href = '/';
    },
    async suggest(field) {
      const q = (this[field] || '').trim();
      if (q.length < 2) {
        this.suggestions[field] = [];
        return;
      }
      try {
        const res = await fetch('/api/contacts/suggest?q=' + encodeURIComponent(q));
        const data = await res.json();
        this.suggestions[field] = data.items || [];
      } catch (_) {
        this.suggestions[field] = [];
      }
    },
    pick(field, item) {
      const email = (item.email || '').trim();
      const label = (item.label || item.name || '').trim();
      if (email && label && !label.includes('@')) {
        this[field] = `${label} <${email}>`;
      } else {
        this[field] = email || label || '';
      }
      this.suggestions[field] = [];
    },
    async fillAi() {
      if (!this.ai || this.aiBusy) return;
      const prompt = (this.aiPrompt || '').trim();
      if (!prompt) {
        this.aiError = 'Saisissez un prompt…';
        return;
      }
      this.aiBusy = true;
      this.aiError = '';
      try {
        const ctx = this.draftContext();
        const res = await fetch('/ai/api/mail', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            prompt,
            kind: this.kind,
            account: this.account || '',
            now: this.localNowLabel(),
            context: ctx.context,
            previous: ctx.previous,
          }),
        });
        const data = await res.json();
        if (data.error) throw new Error(data.error);
        if (data.to) this.to = data.to;
        if (data.subject) this.subject = data.subject;
        if (data.body) {
          this.body = data.body;
          if (this.bodyMode === 'html') {
            this.syncHtmlFromPlain();
            if (this.quill) this.quill.root.innerHTML = this.bodyHtml;
          }
        }
      } catch (e) {
        this.aiError = e.message || String(e);
      } finally {
        this.aiBusy = false;
      }
    },
  };
}

function mapsDirectionsUrl(provider, home, destination) {
  const dest = (destination || '').trim();
  if (!dest) return '';
  const enc = encodeURIComponent(dest);
  const from = (home || '').trim();
  const fromEnc = from ? encodeURIComponent(from) : '';
  switch ((provider || 'google').toLowerCase()) {
    case 'osm':
      return fromEnc
        ? `https://www.openstreetmap.org/directions?engine=fossgis_osrm_car&route=${fromEnc}%3B${enc}`
        : `https://www.openstreetmap.org/search?query=${enc}`;
    case 'apple':
      return fromEnc
        ? `https://maps.apple.com/?saddr=${fromEnc}&daddr=${enc}&dirflg=d`
        : `https://maps.apple.com/?q=${enc}`;
    case 'google':
    default:
      return fromEnc
        ? `https://www.google.com/maps/dir/?api=1&origin=${fromEnc}&destination=${enc}`
        : `https://www.google.com/maps/search/?api=1&query=${enc}`;
  }
}

function eventModal(opts) {
  opts = opts || {};
  const firstCal = opts.defaultCalendar && opts.defaultCalendar !== '__all__'
    ? opts.defaultCalendar
    : '';
  return {
    // Éviter les noms open/close (collision window.open / window.close sous Alpine).
    modalOpen: false,
    _suppressBackdrop: false,
    mode: 'create',
    stay: !!opts.stay,
    busy: false,
    ai: !!opts.ai,
    aiOpen: false,
    aiPrompt: '',
    aiBusy: false,
    aiError: '',
    homeAddress: opts.homeAddress || '',
    mapsProvider: opts.mapsProvider || 'google',
    year: opts.year,
    month: opts.month,
    day: opts.day,
    view: opts.view || 'month',
    id: '',
    calendar: firstCal,
    summary: '',
    startDate: opts.defaultDate || '',
    startTime: '10:00',
    endDate: opts.defaultDate || '',
    endTime: '11:00',
    location: '',
    description: '',
    rrule: 'none',
    get displayTitle() {
      const s = (this.summary || '').trim();
      if (s) return s;
      return this.mode === 'edit' ? 'Modifier l’événement' : 'Nouvel événement';
    },
    mapsLink() {
      return mapsDirectionsUrl(this.mapsProvider, this.homeAddress, this.location);
    },
    openCreate(presetDate) {
      this.mode = 'create';
      this.id = '';
      this.summary = '';
      // @click peut passer l'Event si mal appelé — n'accepter que les strings date
      const d =
        typeof presetDate === 'string' && /^\d{4}-\d{2}-\d{2}/.test(presetDate)
          ? presetDate
          : opts.defaultDate || '';
      this.startDate = d;
      this.endDate = d;
      this.startTime = '10:00';
      this.endTime = '11:00';
      this.location = '';
      this.description = '';
      this.rrule = 'none';
      this.aiPrompt = '';
      this.aiError = '';
      if (firstCal) this.calendar = firstCal;
      this._openModal();
    },
    openEdit(ds) {
      if (!ds) return;
      this.openEditObj({
        id: ds.id,
        calendar: ds.calendar,
        summary: ds.summary,
        date: ds.date,
        endDate: ds.endDate,
        startTime: ds.startTime,
        endTime: ds.endTime,
        location: ds.location,
        description: ds.description,
        rrule: ds.rrule,
      });
    },
    openEditJson(raw) {
      if (!raw) return;
      try {
        const obj = JSON.parse(raw);
        this.openEditObj(obj);
      } catch (e) {
        console.warn('event json', e);
      }
    },
    openEditObj(obj) {
      if (!obj) return;
      this.mode = 'edit';
      this.id = obj.id || '';
      this.calendar = obj.calendar || firstCal;
      this.summary = obj.summary || '';
      this.startDate = obj.date || opts.defaultDate || '';
      this.endDate = obj.endDate || obj.date || opts.defaultDate || '';
      this.startTime = obj.startTime || '10:00';
      this.endTime = obj.endTime || '11:00';
      this.location = obj.location || '';
      this.description = obj.description || '';
      this.rrule = obj.rrule || 'none';
      this.aiPrompt = '';
      this.aiError = '';
      this._openModal();
    },
    _openModal() {
      this._suppressBackdrop = true;
      this.modalOpen = true;
      this.$nextTick(() => {
        if (window.lucide) lucide.createIcons();
        setTimeout(() => {
          this._suppressBackdrop = false;
        }, 200);
      });
    },
    closeModal() {
      this.modalOpen = false;
      this.busy = false;
      this._suppressBackdrop = false;
    },
    onBackdropClick() {
      if (this._suppressBackdrop || !this.modalOpen) return;
      this.closeModal();
    },
    confirmDelete() {
      if (!this.id) return;
      if (!confirm('Supprimer cet événement ?')) return;
      if (this.$refs.delForm) this.$refs.delForm.submit();
    },
    async fillAi() {
      if (!this.ai || this.aiBusy) return;
      const prompt = (this.aiPrompt || '').trim();
      if (!prompt) {
        this.aiError = 'Décrivez l’événement souhaité.';
        return;
      }
      this.aiBusy = true;
      this.aiError = '';
      try {
        const now = (() => {
          try {
            const d = new Date();
            const pad = (n) => String(n).padStart(2, '0');
            const tz = Intl.DateTimeFormat().resolvedOptions().timeZone || '';
            return (
              d.getFullYear() +
              '-' +
              pad(d.getMonth() + 1) +
              '-' +
              pad(d.getDate()) +
              ' ' +
              pad(d.getHours()) +
              ':' +
              pad(d.getMinutes()) +
              (tz ? ' (' + tz + ')' : '')
            );
          } catch (_) {
            return new Date().toISOString();
          }
        })();
        const selected =
          (this.startDate || '') +
          (this.startTime ? ' ' + this.startTime : '') +
          (this.endDate || this.endTime
            ? ' → ' + (this.endDate || this.startDate || '') + (this.endTime ? ' ' + this.endTime : '')
            : '');
        const res = await fetch('/ai/api/event', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            prompt,
            now,
            selected_date: selected.trim(),
          }),
        });
        const data = await res.json();
        if (data.error) throw new Error(data.error);
        const ev = data.event || {};
        if (ev.summary) this.summary = ev.summary;
        if (ev.date) this.startDate = ev.date;
        if (ev.end_date) this.endDate = ev.end_date;
        else if (ev.date) this.endDate = ev.date;
        if (ev.start_time) this.startTime = ev.start_time;
        if (ev.end_time) this.endTime = ev.end_time;
        if (ev.location) this.location = ev.location;
        if (ev.description) this.description = ev.description;
        if (ev.rrule) this.rrule = ev.rrule;
      } catch (e) {
        this.aiError = e.message || String(e);
      } finally {
        this.aiBusy = false;
      }
    },
  };
}

const LUCIDE_FALLBACK = [
  'circle-user', 'mail', 'briefcase', 'home', 'building-2', 'graduation-cap',
  'laptop', 'smartphone', 'globe', 'heart', 'star', 'zap', 'coffee', 'bookmark',
  'shield', 'users', 'inbox', 'send', 'at-sign', 'key', 'lock', 'cloud',
];

window.__lucideIconNames = null;

async function loadLucideIconNames() {
  if (window.__lucideIconNames) return window.__lucideIconNames;
  try {
    const v = hwAssetVersion();
    const res = await fetch(
      `/static/vendor/lucide-tags.json${v ? `?v=${encodeURIComponent(v)}` : ''}`,
    );
    if (!res.ok) throw new Error('tags');
    const tags = await res.json();
    window.__lucideIconNames = Object.keys(tags).sort();
  } catch (_) {
    window.__lucideIconNames = LUCIDE_FALLBACK.slice();
  }
  return window.__lucideIconNames;
}

function accountOrderList(boot) {
  const data = boot && typeof boot === 'object' ? boot : {};
  return {
    items: Array.isArray(data.items) ? data.items.slice() : [],
    selected: data.selected ?? '__all__',
    ntfyMerged: !!data.ntfyMerged,
    ntfySources: Array.isArray(data.ntfySources) ? data.ntfySources : [],
    ntfyMergedItem: data.ntfyMergedItem || {
      id: '__ntfy__',
      label: 'Ntfy',
      icon: 'bell',
      color: '#0ea5e9',
      kind: 'ntfy',
    },
    dragFrom: null,
    get orderValue() {
      return this.items.map((i) => i.id).join(', ');
    },
    syncHidden() {},
    rebuildNtfySlots() {
      const mail = this.items.filter((i) => i.kind !== 'ntfy');
      const ntfy = this.ntfyMerged
        ? this.ntfySources.length
          ? [this.ntfyMergedItem]
          : []
        : this.ntfySources.slice();
      // Conserve la position relative : ntfy à la fin sauf s’ils étaient déjà intercalés —
      // on réinjecte après le premier bloc mail pour rester simple et prévisible.
      this.items = mail.concat(ntfy);
      if (this.selected && String(this.selected).startsWith('__ntfy__')) {
        if (this.ntfyMerged) this.selected = '__ntfy__';
        else if (!this.items.some((i) => i.id === this.selected)) {
          this.selected = this.items.find((i) => i.kind === 'ntfy')?.id || '__all__';
        }
      }
      this.$nextTick(() => {
        if (window.lucide) lucide.createIcons();
      });
    },
    onDragStart(ev, idx) {
      this.dragFrom = idx;
      ev.dataTransfer.effectAllowed = 'move';
      try {
        ev.dataTransfer.setData('text/plain', String(idx));
      } catch (_) {}
      ev.currentTarget.classList.add('is-dragging');
    },
    onDragOver(idx) {
      if (this.dragFrom === null || this.dragFrom === idx) return;
      const next = this.items.slice();
      const [moved] = next.splice(this.dragFrom, 1);
      next.splice(idx, 0, moved);
      this.items = next;
      this.dragFrom = idx;
    },
    onDrop(idx) {
      this.dragFrom = null;
      document.querySelectorAll('.acct-order-item.is-dragging').forEach((el) => {
        el.classList.remove('is-dragging');
      });
    },
    onDragEnd() {
      this.dragFrom = null;
      document.querySelectorAll('.acct-order-item.is-dragging').forEach((el) => {
        el.classList.remove('is-dragging');
      });
    },
    init() {
      this.$nextTick(() => {
        if (window.lucide) lucide.createIcons();
      });
    },
  };
}

function accountAppearance(opts) {
  opts = opts || {};
  return {
    color: opts.color || '#2563eb',
    icon: opts.icon || 'circle-user',
    open: false,
    q: '',
    loading: false,
    all: [],
    shown: [],
    matchCount: 0,
    init() {
      this.refreshIcons();
    },
    async toggle() {
      this.open = !this.open;
      if (this.open) {
        if (!this.all.length) await this.load();
        else this.filter();
        this.refreshIcons();
      }
    },
    async load() {
      this.loading = true;
      this.all = await loadLucideIconNames();
      this.loading = false;
      this.filter();
    },
    filter() {
      const q = (this.q || '').trim().toLowerCase();
      const matched = q
        ? this.all.filter((n) => n.includes(q) || n.replace(/-/g, ' ').includes(q))
        : this.all;
      this.matchCount = matched.length;
      this.shown = matched.slice(0, 240);
      this.refreshIcons();
    },
    pick(name) {
      this.icon = name;
      this.open = false;
      this.refreshIcons();
    },
    statusLabel() {
      if (this.loading) return 'Chargement des icônes Lucide…';
      if (!this.all.length) return '';
      if (this.matchCount > this.shown.length) {
        return `${this.shown.length} / ${this.matchCount} — affinez la recherche`;
      }
      return `${this.matchCount} icône${this.matchCount > 1 ? 's' : ''}`;
    },
    refreshIcons() {
      this.$nextTick(() => {
        if (window.lucide) lucide.createIcons();
      });
    },
  };
}

function quickEventModal() {
  return {
    visible: false,
    busy: false,
    openModal() {
      this.visible = true;
      this.$nextTick(() => {
        if (window.lucide) lucide.createIcons();
      });
    },
    closeModal() {
      this.visible = false;
      this.busy = false;
    },
  };
}

function tbImportForm() {
  return {
    profile: '',
    accounts: [],
    selected: [],
    loading: false,
    error: '',
    init() {
      const sel = this.$el.querySelector('select[name="profile"]');
      if (sel) {
        this.profile = sel.value || '';
        if (this.profile) this.loadAccounts();
      }
    },
    async loadAccounts() {
      if (!this.profile) return;
      this.loading = true;
      this.error = '';
      this.accounts = [];
      this.selected = [];
      try {
        const res = await fetch(
          '/settings/thunderbird/accounts?profile=' + encodeURIComponent(this.profile)
        );
        const data = await res.json();
        if (data.error) throw new Error(data.error);
        this.accounts = data.accounts || [];
        this.selected = this.accounts.map((a) => a.name);
      } catch (e) {
        this.error = e.message || String(e);
      } finally {
        this.loading = false;
      }
    },
  };
}

function messageView(opts) {
  const initialTone = (() => {
    try {
      return localStorage.getItem('himaweb-msg-body-tone') || 'auto';
    } catch (_) {
      return 'auto';
    }
  })();
  const hasAttachments = !!(opts && opts.hasAttachments);
  return {
    accOpen: false,
    attsOpen: hasAttachments,
    _attsAutoOpened: hasAttachments,
    remoteUnlocked: false,
    hasRemote: !!(opts && opts.hasRemote),
    hasAccessory: !!(opts && opts.hasAccessory),
    hasAttachments,
    bodyTone: initialTone,
    get bodyToneClass() {
      if (this.bodyTone === 'light') return 'tone-light';
      if (this.bodyTone === 'dark') return 'tone-dark';
      return '';
    },
    cycleBodyTone() {
      const order = ['auto', 'light', 'dark'];
      const i = order.indexOf(this.bodyTone);
      this.bodyTone = order[(i + 1) % order.length];
      try {
        localStorage.setItem('himaweb-msg-body-tone', this.bodyTone);
      } catch (_) {}
    },
    closeAccPanel() {
      this.accOpen = false;
    },
    positionAccPanel() {
      const panel = this.$refs.accPanel;
      const btn = this.$refs.accBtn;
      if (!panel || !btn) return;
      const r = btn.getBoundingClientRect();
      const width = Math.min(288, window.innerWidth - 16);
      let left = r.left;
      if (left + width > window.innerWidth - 8) {
        left = Math.max(8, window.innerWidth - width - 8);
      }
      panel.style.top = `${Math.round(r.bottom + 6)}px`;
      panel.style.left = `${Math.round(left)}px`;
      panel.style.right = 'auto';
      panel.style.width = `${width}px`;
    },
    toggleAccPanel() {
      this.accOpen = !this.accOpen;
      if (this.accOpen) {
        this.$nextTick(() => this.positionAccPanel());
      }
    },
    toggleRemote() {
      if (!this.hasRemote) {
        this.toggleAccPanel();
        return;
      }
      const body = this.$refs.body || this.$el.querySelector('.msg-body');
      if (!body) return;
      if (this.remoteUnlocked) {
        body.querySelectorAll('img[data-remote-src]').forEach((img) => {
          img.setAttribute(
            'src',
            'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7'
          );
          img.classList.add('remote-img');
          img.classList.remove('remote-loaded', 'remote-failed');
        });
        this.remoteUnlocked = false;
        return;
      }
      body.querySelectorAll('img[data-remote-src]').forEach((img) => {
        const url = img.getAttribute('data-remote-src');
        if (!url) return;
        img.classList.remove('remote-failed');
        img.onerror = () => {
          img.classList.add('remote-failed', 'remote-img');
          img.classList.remove('remote-loaded');
          img.setAttribute(
            'src',
            'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7'
          );
        };
        img.onload = () => {
          img.classList.add('remote-loaded');
          img.classList.remove('remote-img', 'remote-failed');
          img.onerror = null;
          img.onload = null;
        };
        img.setAttribute('src', url);
      });
      this.remoteUnlocked = true;
      if (window.lucide) lucide.createIcons();
    },
  };
}

document.addEventListener('alpine:init', () => {
  if (window.Alpine) {
    Alpine.data('composeForm', composeForm);
    Alpine.data('accountAppearance', accountAppearance);
    Alpine.data('accountOrderList', accountOrderList);
    Alpine.data('quickEventModal', quickEventModal);
    Alpine.data('eventModal', (opts) => eventModal(opts || {}));
    Alpine.data('tbImportForm', tbImportForm);
    Alpine.data('messageView', messageView);
    Alpine.data('inboxSummaryModal', inboxSummaryModal);
  }
});

function inboxSummaryModal() {
  return {
    visible: false,
    busy: false,
    error: '',
    items: [],
    async load() {
      this.visible = true;
      this.busy = true;
      this.error = '';
      this.items = [];
      const trigger = document.getElementById('inbox-ai-summary-btn');
      if (trigger) trigger.classList.add('ai-busy-pulse');
      this.$nextTick(() => {
        if (window.lucide) lucide.createIcons();
      });
      try {
        const mailbox =
          (document.getElementById('current-mailbox') || {}).value || 'Inbox';
        const account = (document.getElementById('current-account') || {}).value || '';
        const rows = [...document.querySelectorAll('#envelope-list .envelope')].slice(0, 15);
        const items = rows.map((el) => ({
          id: el.dataset.id || '',
          account: el.dataset.account || account,
          mailbox: el.dataset.mailbox || mailbox,
          from: (el.querySelector('.from') || {}).textContent || '',
          subject: (el.querySelector('.subject-text') || {}).textContent || '',
        }));
        const res = await fetch('/ai/api/inbox-summary', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ mailbox, account, items, limit: 15 }),
        });
        const data = await res.json();
        if (data.error) throw new Error(data.error);
        this.items = data.items || [];
      } catch (e) {
        this.error = e.message || String(e);
      } finally {
        this.busy = false;
        if (trigger) trigger.classList.remove('ai-busy-pulse');
        this.$nextTick(() => {
          if (window.lucide) lucide.createIcons();
        });
      }
    },
    closeModal() {
      this.visible = false;
      const trigger = document.getElementById('inbox-ai-summary-btn');
      if (trigger) trigger.classList.remove('ai-busy-pulse');
    },
    reply(it) {
      if (!window.HimaWeb) return;
      window.HimaWeb.runContextAction('reply', {
        id: it.id,
        mailbox: it.mailbox || 'Inbox',
        account: it.account || '',
      });
      this.closeModal();
    },
    async archiveRead(it) {
      if (!window.HimaWeb) return;
      const mb = it.mailbox || 'Inbox';
      const acc = it.account || '';
      window.HimaWeb.runContextAction('flag', {
        id: it.id,
        mailbox: mb,
        account: acc,
        seen: '1',
      });
      const items = [{ id: it.id, mailbox: mb, account: acc, messageId: '' }];
      try {
        await window.HimaWeb.moveMailsToFolder(items, 'Archive', acc);
      } catch (_) {
        /* Archive peut ne pas exister — lu suffit */
      }
      this.items = this.items.filter((x) => x.id !== it.id);
    },
    remove(it) {
      if (!window.HimaWeb) return;
      window.HimaWeb.runContextAction('delete', {
        id: it.id,
        mailbox: it.mailbox || 'Inbox',
        account: it.account || '',
      });
      this.items = this.items.filter((x) => x.id !== it.id);
    },
  };
}

window.HimaWeb = {
  messageView,

  openInboxSummary() {
    let host = document.getElementById('inbox-summary-host');
    if (!host) {
      host = document.createElement('div');
      host.id = 'inbox-summary-host';
      document.body.appendChild(host);
    } else if (host.parentElement !== document.body) {
      // Sortir du layout mail (overflow / stacking context) pour la modal fixed
      document.body.appendChild(host);
    }
    let root = host.querySelector('[data-inbox-summary]');
    if (!root) {
      host.innerHTML = `
<div data-inbox-summary class="modal-backdrop inbox-summary-backdrop" x-data="inboxSummaryModal"
     x-show="visible" x-cloak
     @keydown.escape.window="closeModal()"
     @click.self="closeModal()">
  <div class="modal-panel inbox-summary-panel" @click.stop>
    <div class="flex items-center justify-between mb-3">
      <h2 class="font-display text-xl">Résumé inbox</h2>
      <button type="button" class="icon-btn" @click="closeModal()" title="Fermer"><i data-lucide="x"></i></button>
    </div>
    <p class="muted tiny mb-3" x-show="busy">Analyse en cours…</p>
    <p class="offline-strip mb-3" x-show="error" x-text="error"></p>
    <ul class="inbox-summary-list" x-show="!busy && !error">
      <template x-for="it in items" :key="it.id">
        <li class="inbox-summary-row">
          <div class="inbox-summary-meta">
            <strong class="inbox-summary-from" x-text="it.from || '—'"></strong>
            <span class="muted tiny" x-text="it.subject || ''"></span>
            <p class="inbox-summary-text" x-text="it.summary || ''"></p>
          </div>
          <div class="inbox-summary-actions">
            <button type="button" class="icon-btn" title="Répondre" @click="reply(it)"><i data-lucide="reply"></i></button>
            <button type="button" class="icon-btn" title="Archiver + lu" @click="archiveRead(it)"><i data-lucide="archive"></i></button>
            <button type="button" class="icon-btn" title="Supprimer" @click="remove(it)"><i data-lucide="trash-2"></i></button>
          </div>
        </li>
      </template>
    </ul>
    <p class="muted tiny" x-show="!busy && !error && items.length === 0">Aucun message à résumer.</p>
  </div>
</div>`;
      if (window.Alpine && typeof Alpine.initTree === 'function') {
        Alpine.initTree(host);
      }
      root = host.querySelector('[data-inbox-summary]');
    }
    // Différer l’ouverture : le clic d’ouverture ne doit pas être pris pour un click.outside
    const start = () => {
      const data =
        (root && root._x_dataStack && root._x_dataStack[0]) ||
        (root && window.Alpine && Alpine.$data(root));
      if (data && typeof data.load === 'function') data.load();
      if (window.lucide) lucide.createIcons();
    };
    if (window.Alpine && typeof Alpine.nextTick === 'function') {
      Alpine.nextTick(start);
    } else {
      setTimeout(start, 0);
    }
  },

  openAttPreview(el) {
    if (!el) return;
    const previewUrl = el.getAttribute('data-preview-url');
    const downloadUrl = el.getAttribute('data-download-url') || previewUrl;
    const filename = el.getAttribute('data-filename') || 'pièce jointe';
    const mime = (el.getAttribute('data-mime') || '').toLowerCase();
    const modal = document.getElementById('att-preview-modal');
    const body = document.getElementById('att-preview-body');
    const title = document.getElementById('att-preview-title');
    const dl = document.getElementById('att-preview-download');
    if (!modal || !body || !title || !dl || !previewUrl) return;
    title.textContent = filename;
    dl.href = downloadUrl;
    dl.setAttribute('download', filename);
    body.innerHTML = '';
    if (mime.startsWith('image/') || /\.(png|jpe?g|gif|webp|svg)$/i.test(filename)) {
      const img = document.createElement('img');
      img.src = previewUrl;
      img.alt = filename;
      img.className = 'att-preview-img';
      body.appendChild(img);
    } else {
      const frame = document.createElement('iframe');
      frame.src = previewUrl;
      frame.title = filename;
      frame.className = 'att-preview-frame';
      body.appendChild(frame);
    }
    modal.hidden = false;
    if (window.lucide) lucide.createIcons();
    const onKey = (ev) => {
      if (ev.key === 'Escape') {
        document.removeEventListener('keydown', onKey);
        this.closeAttPreview();
      }
    };
    document.addEventListener('keydown', onKey);
  },

  closeAttPreview() {
    const modal = document.getElementById('att-preview-modal');
    const body = document.getElementById('att-preview-body');
    if (body) body.innerHTML = '';
    if (modal) modal.hidden = true;
  },


  _lastUnreadTotal: null,
  _folderClicksBound: false,
  _folderTreeBound: false,
  _resizeBound: false,
  _confirmDelete: true,
  _mailUndoStack: [],
  _mailUndoMax: 20,

  settingsSaved(form, evt) {
    if (!form) return;
    if (evt && evt.detail && evt.detail.successful === false) return;
    this.applyUiFromForm(form);
    const el = form.querySelector('.settings-saved');
    if (!el) return;
    el.hidden = false;
    clearTimeout(form._savedTimer);
    form._savedTimer = setTimeout(() => {
      el.hidden = true;
    }, 1600);
  },

  applyUiFromForm(form) {
    if (!form) return;
    const root = document.documentElement;
    const num = (name, fallback) => {
      const el = form.querySelector(`[name="${name}"]`);
      if (!el || el.value === '' || el.value == null) return fallback;
      const n = Number(el.value);
      return Number.isFinite(n) ? n : fallback;
    };
    const font = num('ui_font_scale', 1);
    const radius = num('ui_radius', 2);
    const space = num('ui_space', 1);
    const rail = num('ui_rail', 260);
    const list = num('ui_list', 380);
    root.style.setProperty('--font-scale', font.toFixed(2));
    root.style.setProperty('--radius', `${Math.round(radius)}px`);
    root.style.setProperty('--ui-space', space.toFixed(2));
    root.style.setProperty('--rail', `${Math.round(rail)}px`);
    root.style.setProperty('--list', `${Math.round(list)}px`);

    const setLabel = (name, text) => {
      const lab = form.querySelector(`[data-ui-label="${name}"]`);
      if (lab) lab.textContent = text;
    };
    setLabel('ui_font_scale', `Taille du texte (${font.toFixed(2)})`);
    setLabel('ui_radius', `Coins arrondis (${Math.round(radius)} px)`);
    setLabel('ui_space', `Densité / marges (${space.toFixed(2)})`);
  },

  bindUiPreview() {
    if (this._uiPreviewBound) return;
    this._uiPreviewBound = true;
    document.addEventListener('input', (ev) => {
      const t = ev.target;
      if (!t || !t.name) return;
      const form = t.closest('form.settings-autosave');
      if (!form) return;
      const action = form.getAttribute('hx-post') || form.getAttribute('action') || '';
      if (!action.includes('/settings/ui')) return;
      if (
        ![
          'ui_font_scale',
          'ui_radius',
          'ui_space',
          'ui_rail',
          'ui_list',
        ].includes(t.name)
      ) {
        return;
      }
      this.applyUiFromForm(form);
    });
  },

  async loadPrefs() {
    try {
      const res = await fetch('/api/prefs', { headers: { Accept: 'application/json' } });
      const data = await res.json();
      if (typeof data.confirm_delete === 'boolean') {
        this._confirmDelete = data.confirm_delete;
      }
    } catch (_) {}
  },

  async setConfirmDelete(enabled) {
    this._confirmDelete = !!enabled;
    try {
      const body = new URLSearchParams();
      if (enabled) body.set('confirm_delete', '1');
      await fetch('/settings/confirm-delete', {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: body.toString(),
      });
    } catch (_) {}
  },

  confirmDelete(message) {
    return new Promise((resolve) => {
      if (!this._confirmDelete) {
        resolve(true);
        return;
      }
      let backdrop = document.getElementById('himaweb-confirm');
      if (backdrop) backdrop.remove();
      backdrop = document.createElement('div');
      backdrop.id = 'himaweb-confirm';
      backdrop.className = 'confirm-backdrop';
      backdrop.innerHTML = `
        <div class="modal-panel confirm-panel" role="dialog" aria-modal="true" aria-labelledby="confirm-title">
          <h2 id="confirm-title" class="font-display text-xl">Confirmation</h2>
          <p class="confirm-message"></p>
          <label class="toggle-row confirm-dont-ask">
            <span>Ne plus demander</span>
            <input type="checkbox" class="toggle" id="confirm-dont-ask" />
          </label>
          <div class="btn-row confirm-actions">
            <button type="button" class="btn ghost" data-confirm="no">Annuler</button>
            <button type="button" class="btn danger" data-confirm="yes">Supprimer</button>
          </div>
        </div>`;
      backdrop.querySelector('.confirm-message').textContent = message;
      const finish = async (ok) => {
        const dont = backdrop.querySelector('#confirm-dont-ask');
        if (ok && dont && dont.checked) {
          await this.setConfirmDelete(false);
        }
        backdrop.remove();
        resolve(ok);
      };
      backdrop.addEventListener('click', (ev) => {
        if (ev.target === backdrop) finish(false);
      });
      backdrop.querySelector('[data-confirm="no"]').onclick = () => finish(false);
      backdrop.querySelector('[data-confirm="yes"]').onclick = () => finish(true);
      document.addEventListener(
        'keydown',
        function onKey(ev) {
          if (ev.key === 'Escape') {
            document.removeEventListener('keydown', onKey);
            finish(false);
          } else if (ev.key === 'Enter') {
            document.removeEventListener('keydown', onKey);
            finish(true);
          }
        },
        { once: true }
      );
      document.body.appendChild(backdrop);
      if (window.lucide) lucide.createIcons();
      const yes = backdrop.querySelector('[data-confirm="yes"]');
      if (yes) yes.focus();
    });
  },

  pushMailUndo(entry) {
    if (!entry || !entry.items || !entry.items.length) return;
    const usable = entry.items.filter(
      (it) => it.messageId && it.fromMailbox && it.toMailbox && !it.permanent
    );
    if (!usable.length) return;
    this._mailUndoStack.push({ type: entry.type || 'delete', items: usable });
    while (this._mailUndoStack.length > this._mailUndoMax) {
      this._mailUndoStack.shift();
    }
    this.showUndoToast(usable.length);
  },

  showUndoToast(n) {
    let toast = document.getElementById('mail-undo-toast');
    if (!toast) {
      toast = document.createElement('div');
      toast.id = 'mail-undo-toast';
      toast.className = 'mail-undo-toast';
      document.body.appendChild(toast);
    }
    const label = n === 1 ? '1 message' : `${n} messages`;
    toast.innerHTML = `<span>${label} — Ctrl+Z pour annuler</span>
      <button type="button" class="linkish" id="mail-undo-btn">Annuler</button>`;
    const btn = document.getElementById('mail-undo-btn');
    if (btn) btn.onclick = () => this.undoLastMailOp();
    clearTimeout(this._undoToastTimer);
    this._undoToastTimer = setTimeout(() => {
      if (toast) toast.remove();
    }, 8000);
  },

  reloadEnvelopeList() {
    const listEl = document.getElementById('envelope-list');
    if (!listEl || !window.htmx) return;
    const mb =
      (document.getElementById('current-mailbox') &&
        document.getElementById('current-mailbox').value) ||
      'Inbox';
    const ac =
      (document.getElementById('current-account') &&
        document.getElementById('current-account').value) ||
      '';
    const sortEl = document.getElementById('mail-sort');
    const sort = (sortEl && sortEl.value) || 'date_desc';
    let url =
      '/partials/envelopes?mailbox=' +
      encodeURIComponent(mb) +
      '&sort=' +
      encodeURIComponent(sort) +
      '&page=1';
    if (ac) url += '&account=' + encodeURIComponent(ac);
    window.htmx.ajax('GET', url, { target: '#envelope-list', swap: 'innerHTML' });
  },

  async undoLastMailOp() {
    const entry = this._mailUndoStack.pop();
    const toast = document.getElementById('mail-undo-toast');
    if (toast) toast.remove();
    if (!entry || !entry.items || !entry.items.length) return;
    HWProgress.begin();
    try {
      const res = await fetch('/api/mail/undo', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
        body: JSON.stringify({
          items: entry.items.map((it) => ({
            account: it.account || '',
            message_id: it.messageId,
            from_mailbox: it.fromMailbox,
            to_mailbox: it.toMailbox,
          })),
        }),
      });
      const data = await res.json();
      if (data && data.errors && data.errors.length) {
        console.warn('undo errors', data.errors);
        alert(data.errors.join('\n'));
      }
      if (data && data.restored > 0) {
        // Laisser IMAP propager le move avant de recharger la liste
        await new Promise((r) => setTimeout(r, 400));
        this.reloadEnvelopeList();
        this.reloadSidebar();
        this.pollUnread();
        const pane = document.getElementById('message-pane');
        if (pane) {
          pane.innerHTML =
            '<div class="empty-read"><i data-lucide="mail-open"></i><p>Sélectionnez un message</p></div>';
          hwRenderIcons(pane);
        }
      }
    } catch (e) {
      console.error(e);
      alert('Échec de l’annulation');
    } finally {
      HWProgress.end();
    }
  },

  bindDeleteFormConfirm() {
    if (this._deleteFormBound) return;
    this._deleteFormBound = true;
    document.addEventListener(
      'submit',
      async (ev) => {
        const form = ev.target;
        if (!form || !form.matches) return;
        if (!form.matches('form[hx-post="/partials/message/delete"]')) return;
        ev.preventDefault();
        ev.stopPropagation();
        const ok = await this.confirmDelete('Supprimer ce message ?');
        if (!ok) return;
        const fd = new FormData(form);
        const id = String(fd.get('id') || '');
        const mailbox = String(fd.get('mailbox') || 'Inbox');
        const account = String(fd.get('account') || '');
        if (!id) return;
        const env = document.querySelector(
          `.envelope[data-id="${CSS.escape(id)}"]${account ? `[data-account="${CSS.escape(account)}"]` : ''}`
        );
        this.deleteMailsApi([
          {
            id,
            mailbox,
            account,
            messageId: (env && env.dataset.messageId) || '',
          },
        ]);
      },
      true
    );
  },

  markEnvelopeRead(id, account) {
    const acc = account || '';
    let changed = false;
    this.envelopeRoots().forEach((root) => {
      root.querySelectorAll('.envelope.unread').forEach((el) => {
        if (el.dataset.id !== String(id)) return;
        // Si account est fourni, filtrer ; sinon matcher l’id seul (NTFY, etc.)
        if (acc && (el.dataset.account || '') !== acc) return;
        el.classList.remove('unread');
        changed = true;
        el.querySelectorAll('.from strong').forEach((s) => {
          const parent = s.parentNode;
          while (s.firstChild) parent.insertBefore(s.firstChild, s);
          s.remove();
        });
      });
    });
    if (changed) this.bumpUnread(-1);
    this.scheduleMailRefresh({ envelopes: false, sidebar: false, unread: true, unreadDelay: 900 });
  },

  applyEnvelopeSeen(id, account, seen) {
    const acc = account || '';
    let changed = false;
    this.envelopeRoots().forEach((root) => {
      root.querySelectorAll('.envelope').forEach((el) => {
        if (el.dataset.id !== String(id)) return;
        if (acc && (el.dataset.account || '') !== acc) return;
        const wasUnread = el.classList.contains('unread');
        if (seen) {
          if (!wasUnread) return;
          el.classList.remove('unread');
          changed = true;
          el.querySelectorAll('.from strong').forEach((s) => {
            const parent = s.parentNode;
            while (s.firstChild) parent.insertBefore(s.firstChild, s);
            s.remove();
          });
        } else {
          if (wasUnread) return;
          el.classList.add('unread');
          changed = true;
          const from = el.querySelector('.from');
          if (from && !from.querySelector('strong')) {
            const strong = document.createElement('strong');
            while (from.firstChild) strong.appendChild(from.firstChild);
            from.appendChild(strong);
          }
        }
      });
    });
    if (changed) this.bumpUnread(seen ? -1 : 1);
    this.scheduleMailRefresh({ envelopes: false, sidebar: false, unread: true, unreadDelay: 900 });
  },

  bumpUnread(delta) {
    if (!delta) return;
    let n = this._lastUnreadTotal;
    if (n === null || n === undefined) {
      const badge = document.getElementById('unread-badge');
      n = badge && !badge.hidden ? parseInt(badge.textContent, 10) || 0 : 0;
    }
    n = Math.max(0, n + delta);
    this._lastUnreadTotal = n;
    const badge = document.getElementById('unread-badge');
    if (badge) {
      if (n > 0) {
        badge.hidden = false;
        badge.textContent = n > 99 ? '99+' : String(n);
      } else {
        badge.hidden = true;
        badge.textContent = '0';
      }
    }
    this.setUnreadFavicon(n);
    this.setUnreadTitle(n);
  },

  onQuickEventCreated() {
    const root = document.querySelector('.side-widget-inner');
    if (root && root._x_dataStack && root._x_dataStack[0]) {
      try {
        const d = root._x_dataStack[0];
        if (typeof d.closeModal === 'function') d.closeModal();
      } catch (_) {}
    }
    const body = document.querySelector('#side-widget .side-widget-body');
    if (body && window.htmx) {
      window.htmx.ajax('GET', '/partials/side-widget', { target: body, swap: 'innerHTML' });
    }
  },

  openCompose(url) {
    const layer = document.getElementById('compose-layer');
    if (!layer || !window.htmx) {
      window.location.href = url;
      return;
    }
    const u = new URL(url, window.location.origin);
    u.searchParams.set('embed', '1');
    window.htmx.ajax('GET', u.pathname + u.search, {
      target: '#compose-layer',
      swap: 'innerHTML',
    });
  },

  closeComposeOverlay() {
    const layer = document.getElementById('compose-layer');
    if (!layer || !layer.innerHTML.trim()) return false;
    layer.innerHTML = '';
    return true;
  },

  scheduleMailRefresh({ envelopes = false, sidebar = true, unread = true, unreadDelay = 0 } = {}) {
    clearTimeout(this._mailRefreshTimer);
    this._mailRefreshTimer = setTimeout(() => {
      if (envelopes) this.reloadEnvelopes();
      if (sidebar) this.reloadSidebar();
      if (unread) {
        const run = () => {
          this.pollUnread().finally(() => {
            if (unreadDelay > 0) {
              clearTimeout(this._unreadConfirmTimer);
              this._unreadConfirmTimer = setTimeout(() => this.pollUnread(), unreadDelay);
            }
          });
        };
        run();
      }
    }, 250);
  },

  reloadSidebar() {
    const sidebar = document.getElementById('sidebar');
    if (!sidebar || !window.htmx) return;
    const mb =
      (document.getElementById('current-mailbox') &&
        document.getElementById('current-mailbox').value) ||
      'Inbox';
    const ac =
      (document.getElementById('current-account') &&
        document.getElementById('current-account').value) ||
      '';
    let url = '/partials/sidebar?mailbox=' + encodeURIComponent(mb);
    if (ac) url += '&account=' + encodeURIComponent(ac);
    window.htmx.ajax('GET', url, { target: '#sidebar', swap: 'innerHTML' });
  },

  bindColumnResize() {
    if (this._resizeBound) return;
    this._resizeBound = true;
    let drag = null;
    const root = () => document.documentElement;

    const onMove = (ev) => {
      if (!drag) return;
      const dx = ev.clientX - drag.startX;
      if (drag.kind === 'rail') {
        const w = Math.min(420, Math.max(160, drag.startW + dx));
        root().style.setProperty('--rail', w + 'px');
        drag.current = w;
      } else {
        const w = Math.min(560, Math.max(240, drag.startW + dx));
        root().style.setProperty('--list', w + 'px');
        drag.current = w;
      }
    };
    const onUp = () => {
      if (!drag) return;
      document.querySelectorAll('.col-resizer').forEach((r) => r.classList.remove('dragging'));
      const kind = drag.kind;
      const val = drag.current;
      drag = null;
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      if (val == null) return;
      const body = new URLSearchParams();
      if (kind === 'rail') body.set('rail', String(Math.round(val)));
      else body.set('list', String(Math.round(val)));
      fetch('/settings/ui/sizes', { method: 'POST', body, headers: { 'Content-Type': 'application/x-www-form-urlencoded' } }).catch(() => {});
    };
    document.addEventListener('pointermove', onMove);
    document.addEventListener('pointerup', onUp);
    document.addEventListener('pointerdown', (ev) => {
      const handle = ev.target.closest('.col-resizer');
      if (!handle) return;
      ev.preventDefault();
      const kind = handle.getAttribute('data-resize');
      const cs = getComputedStyle(root());
      const startW = parseFloat(cs.getPropertyValue(kind === 'rail' ? '--rail' : '--list')) || (kind === 'rail' ? 260 : 380);
      drag = { kind, startX: ev.clientX, startW, current: startW };
      handle.classList.add('dragging');
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
    });
  },

  /* --- Retour visuel de chargement ------------------------------------ */

  /** Lignes fantômes imitant la liste d'enveloppes. */
  listSkeletonHtml(rows) {
    const widths = ['w-60', 'w-80', 'w-40', 'w-100', 'w-60'];
    let out = '<div class="hw-skeleton-list" aria-hidden="true">';
    for (let i = 0; i < (rows || 7); i++) {
      out +=
        '<div class="hw-skeleton-row">' +
        '<div class="hw-skeleton hw-skeleton-avatar"></div>' +
        '<div class="hw-skeleton-lines">' +
        '<div class="hw-skeleton hw-skeleton-line w-40"></div>' +
        `<div class="hw-skeleton hw-skeleton-line ${widths[i % widths.length]}"></div>` +
        '</div></div>';
    }
    return `${out}</div>`;
  },

  messageSkeletonHtml() {
    return (
      '<div class="hw-skeleton-message" aria-hidden="true">' +
      '<div class="hw-skeleton hw-skeleton-title"></div>' +
      '<div class="hw-skeleton hw-skeleton-meta"></div>' +
      '<div class="hw-skeleton-body">' +
      '<div class="hw-skeleton hw-skeleton-line w-100"></div>' +
      '<div class="hw-skeleton hw-skeleton-line w-100"></div>' +
      '<div class="hw-skeleton hw-skeleton-line w-80"></div>' +
      '<div class="hw-skeleton hw-skeleton-line w-100"></div>' +
      '<div class="hw-skeleton hw-skeleton-line w-60"></div>' +
      '</div></div>'
    );
  },

  /** Marque une zone comme en cours de mise à jour (lecteurs d'écran inclus). */
  setPaneBusy(el, busy) {
    if (!el) return;
    if (busy) el.setAttribute('aria-busy', 'true');
    else el.removeAttribute('aria-busy');
  },

  showMessageLoading() {
    const pane = document.getElementById('message-pane');
    if (!pane) return;
    this.setPaneBusy(pane, true);
    pane.innerHTML = this.messageSkeletonHtml();
  },

  showListLoading() {
    const list = document.getElementById('envelope-list');
    if (!list) return;
    this.setPaneBusy(list, true);
    list.removeAttribute('data-stale');
    list.innerHTML = this.listSkeletonHtml(8);
  },

  /** Surligne l'enveloppe ouverte dès le clic, avant la réponse serveur. */
  markEnvelopeOpening(env) {
    const list = document.getElementById('envelope-list') || document;
    list.querySelectorAll('.envelope.is-opening').forEach((el) => {
      if (el !== env) el.classList.remove('is-opening');
    });
    if (env) env.classList.add('is-opening');
  },

  clearEnvelopeOpening() {
    document
      .querySelectorAll('.envelope.is-opening')
      .forEach((el) => el.classList.remove('is-opening'));
  },

  /** Grise les enveloppes visées par une suppression / un déplacement en cours. */
  setEnvelopesPending(items, on) {
    (items || []).forEach((it) => {
      if (!it || !it.id) return;
      const acc = it.account || '';
      const sel = `.envelope[data-id="${cssEsc(String(it.id))}"]${
        acc ? `[data-account="${cssEsc(acc)}"]` : ''
      }`;
      document.querySelectorAll(sel).forEach((el) => el.classList.toggle('is-pending', !!on));
    });
  },

  /**
   * Les formulaires POST classiques (envoi, sync, CRUD calendrier/contacts)
   * provoquent une navigation complète : sans retour visuel, le clic semble
   * sans effet pendant toute la requête.
   */
  bindFormBusy() {
    if (this._formBusyBound) return;
    this._formBusyBound = true;
    document.addEventListener('submit', (ev) => {
      const form = ev.target;
      if (!form || !form.matches || ev.defaultPrevented) return;
      // HTMX gère déjà son propre cycle d'indicateurs.
      if (form.hasAttribute('hx-post') || form.hasAttribute('hx-get')) return;

      const btn =
        (ev.submitter && ev.submitter.tagName === 'BUTTON' ? ev.submitter : null) ||
        form.querySelector('button[type="submit"], button:not([type])');
      if (!btn || btn.dataset.hwBusy === '1') return;

      btn.dataset.hwBusy = '1';
      btn.classList.add('hw-busy');
      const spinner = document.createElement('span');
      spinner.className = 'hw-spinner';
      spinner.setAttribute('aria-hidden', 'true');
      btn.insertBefore(spinner, btn.firstChild);
      HWProgress.begin();

      // La page va être remplacée ; ce filet ne sert qu'en cas d'échec réseau.
      setTimeout(() => {
        if (btn.dataset.hwBusy !== '1') return;
        delete btn.dataset.hwBusy;
        btn.classList.remove('hw-busy');
        spinner.remove();
        HWProgress.end();
      }, 30000);
    });
  },

  envelopeRoots() {
    const roots = [
      document.getElementById('envelope-list'),
      document.getElementById('search-results'),
    ].filter(Boolean);
    return roots.length ? roots : [document];
  },

  /**
   * Au survol, précharge le corps dans le cache SQLite (pool de fond).
   * Plafonné à 2 requêtes simultanées, délai 150 ms, sans barre de progression.
   */
  bindMessagePrefetch() {
    if (this._prefetchBound) return;
    this._prefetchBound = true;
    this._prefetchSeen = new Set();
    this._prefetchInflight = 0;
    this._prefetchTimer = null;
    this._prefetchUrl = null;

    const schedule = (env) => {
      const url = env.getAttribute('hx-get');
      if (!url || url === this._prefetchUrl) return;
      if (this._prefetchTimer) clearTimeout(this._prefetchTimer);
      this._prefetchUrl = url;
      this._prefetchTimer = setTimeout(() => {
        this._prefetchTimer = null;
        this.runMessagePrefetch(url);
      }, 150);
    };

    document.addEventListener('pointerover', (ev) => {
      const env = ev.target.closest && ev.target.closest('.envelope[hx-get]');
      if (!env) return;
      const from = ev.relatedTarget;
      if (from && env.contains(from)) return;
      schedule(env);
    });
    document.addEventListener('pointerout', (ev) => {
      const env = ev.target.closest && ev.target.closest('.envelope[hx-get]');
      if (!env) return;
      const to = ev.relatedTarget;
      if (to && env.contains(to)) return;
      if (this._prefetchTimer) {
        clearTimeout(this._prefetchTimer);
        this._prefetchTimer = null;
        this._prefetchUrl = null;
      }
    });
  },

  runMessagePrefetch(url) {
    if (!url || this._prefetchInflight >= 2) return;
    if (this._prefetchSeen.has(url)) return;
    this._prefetchSeen.add(url);
    if (this._prefetchSeen.size > 80) {
      const first = this._prefetchSeen.values().next().value;
      this._prefetchSeen.delete(first);
    }
    this._prefetchInflight += 1;
    const sep = url.includes('?') ? '&' : '?';
    fetch(`${url}${sep}prefetch=1`, { credentials: 'same-origin', headers: { Accept: 'text/plain' } })
      .catch(() => {
        this._prefetchSeen.delete(url);
      })
      .finally(() => {
        this._prefetchInflight = Math.max(0, this._prefetchInflight - 1);
      });
  },

  bindFolderClicks() {
    if (this._folderClicksBound) return;
    this._folderClicksBound = true;
    document.addEventListener('click', (ev) => {
      const el = ev.target.closest('.folder-link[data-label], .folder-link-text[data-label]');
      if (!el || el.classList.contains('folder-acct-label')) return;
      const label = el.getAttribute('data-label');
      const title = document.getElementById('list-mailbox-label');
      if (title && label) title.textContent = label;
      const iconName = el.getAttribute('data-folder-icon') || 'folder';
      const iconEl = document.getElementById('list-mailbox-icon');
      if (iconEl) {
        iconEl.setAttribute('data-lucide', iconName);
        if (window.lucide) lucide.createIcons();
      }
      const color = el.getAttribute('data-account-color');
      const listTitle = document.getElementById('list-title');
      if (listTitle) {
        if (el.classList.contains('folder-merged') || !color) {
          listTitle.style.setProperty('--list-title-color', 'var(--accent)');
        } else {
          listTitle.style.setProperty('--list-title-color', color);
        }
      }
      const mb = document.getElementById('current-mailbox');
      const ac = document.getElementById('current-account');
      try {
        const raw = el.getAttribute('hx-get') || el.getAttribute('href') || '';
        const u = new URL(raw, window.location.origin);
        if (mb) mb.value = u.searchParams.get('mailbox') || 'Inbox';
        if (ac) ac.value = u.searchParams.get('account') || '';
      } catch (_) {}
      document.querySelectorAll('.folder-item').forEach((f) => f.classList.remove('active'));
      document.querySelectorAll('.folder-link').forEach((f) => f.classList.remove('active'));
      const row = el.closest('.folder-item');
      if (row) row.classList.add('active');
      else el.classList.add('active');
      const pane = document.getElementById('message-pane');
      if (pane) {
        pane.innerHTML =
          '<div class="empty-read"><i data-lucide="mail-open"></i><p>Sélectionnez un message</p></div>';
        if (window.lucide) lucide.createIcons(pane);
      }
      // La liste va être remplacée par HTMX : montrer l'attente tout de suite.
      this.clearEnvelopeOpening();
      this.showListLoading();
    });
  },

  bindFolderTree(force) {
    const nav = document.getElementById('folder-nav');
    if (!nav) return;

    if (!this._folderTreeBound) {
      this._folderTreeBound = true;
      document.addEventListener('click', (ev) => {
        const btn = ev.target.closest('.folder-twist');
        if (!btn) return;
        const tree = btn.closest('#folder-nav');
        if (!tree) return;
        ev.preventDefault();
        ev.stopPropagation();
        const path = btn.getAttribute('data-twist');
        if (!path) return;
        const open = btn.getAttribute('aria-expanded') === 'true';
        window.HimaWeb.setFolderExpanded(tree, path, !open);
      });
    }

    // Replier tout (sauf profondeur 0), puis restaurer l’état mémorisé + chemin actif
    nav.querySelectorAll('.folder-row[data-depth]').forEach((row) => {
      const depth = parseInt(row.dataset.depth || '0', 10);
      row.classList.toggle('is-folded', depth > 0);
    });
    nav.querySelectorAll('.folder-twist').forEach((btn) => {
      btn.setAttribute('aria-expanded', 'false');
      btn.setAttribute('aria-label', 'Déplier');
      btn.classList.remove('open');
    });

    const remembered = this.getExpandedFolders();
    remembered.forEach((path) => {
      if (nav.querySelector(`.folder-row[data-tree-id="${cssEsc(path)}"]`)) {
        this.setFolderExpanded(nav, path, true, true);
      }
    });

    const current = nav.getAttribute('data-current') || '';
    const active = nav.querySelector('.folder-item.active');
    const activeRow = active && active.closest('.folder-row');
    if (activeRow) {
      let parentId = activeRow.dataset.parent || '';
      while (parentId) {
        this.setFolderExpanded(nav, parentId, true, true);
        const parentRow = nav.querySelector(`.folder-row[data-tree-id="${cssEsc(parentId)}"]`);
        parentId = parentRow ? parentRow.dataset.parent || '' : '';
      }
    } else if (current) {
      const parts = current.split('/');
      let acc = '';
      for (let i = 0; i < parts.length - 1; i++) {
        acc = acc ? acc + '/' + parts[i] : parts[i];
        const row = [...nav.querySelectorAll('.folder-row')].find((r) => {
          const id = r.dataset.treeId || '';
          return id === acc || id.endsWith('::' + acc);
        });
        if (row) this.setFolderExpanded(nav, row.dataset.treeId, true, true);
      }
    }

    // Sync mémoire avec l’état réel (chemins encore présents)
    const openIds = [];
    nav.querySelectorAll('.folder-twist[aria-expanded="true"]').forEach((btn) => {
      const path = btn.getAttribute('data-twist');
      if (path) openIds.push(path);
    });
    this.saveExpandedFolders(openIds);

    if (window.lucide) lucide.createIcons();
    void force;
  },

  getExpandedFolders() {
    try {
      const raw = sessionStorage.getItem('himaweb-folder-expand');
      const arr = raw ? JSON.parse(raw) : [];
      return Array.isArray(arr) ? arr : [];
    } catch (_) {
      return [];
    }
  },

  saveExpandedFolders(ids) {
    try {
      sessionStorage.setItem('himaweb-folder-expand', JSON.stringify([...new Set(ids)]));
    } catch (_) {}
  },

  setFolderExpanded(nav, path, expanded, skipPersist) {
    if (!nav || !path) return;
    const twist = [...nav.querySelectorAll('.folder-twist')].find(
      (b) => b.getAttribute('data-twist') === path
    );
    if (twist) {
      twist.setAttribute('aria-expanded', expanded ? 'true' : 'false');
      twist.setAttribute('aria-label', expanded ? 'Replier' : 'Déplier');
      twist.classList.toggle('open', expanded);
    }
    nav.querySelectorAll('.folder-row').forEach((row) => {
      if (row.dataset.parent === path) {
        row.classList.toggle('is-folded', !expanded);
        if (!expanded) {
          window.HimaWeb.setFolderExpanded(nav, row.dataset.treeId, false, true);
        }
      }
    });
    if (!skipPersist) {
      const openIds = [];
      nav.querySelectorAll('.folder-twist[aria-expanded="true"]').forEach((btn) => {
        const p = btn.getAttribute('data-twist');
        if (p) openIds.push(p);
      });
      this.saveExpandedFolders(openIds);
    }
  },

  toggleRailCompact() {
    const shell = document.getElementById('mail-root') || document.querySelector('.gmail-shell');
    if (!shell) return;
    const on = !shell.classList.contains('rail-compact');
    shell.classList.toggle('rail-compact', on);
    try {
      localStorage.setItem('himaweb-rail-compact', on ? '1' : '0');
    } catch (_) {}
    const btn = document.querySelector('.rail-compact-btn');
    if (btn) {
      btn.title = on ? 'Étendre la colonne' : 'Compacter la colonne';
      btn.setAttribute('aria-label', btn.title);
    }
    if (window.lucide) lucide.createIcons();
  },

  applyRailCompactFromStorage() {
    const shell = document.getElementById('mail-root') || document.querySelector('.gmail-shell');
    if (!shell) return;
    let on = false;
    try {
      on = localStorage.getItem('himaweb-rail-compact') === '1';
    } catch (_) {}
    shell.classList.toggle('rail-compact', on);
    const btn = document.querySelector('.rail-compact-btn');
    if (btn) {
      btn.title = on ? 'Étendre la colonne' : 'Compacter la colonne';
      btn.setAttribute('aria-label', btn.title);
    }
  },

  async pollUnread() {
    try {
      const res = await fetch('/api/mail/unread');
      const data = await res.json();
      const total = data.total || 0;
      const badge = document.getElementById('unread-badge');
      if (badge) {
        if (total > 0) {
          badge.hidden = false;
          badge.textContent = total > 99 ? '99+' : String(total);
        } else {
          badge.hidden = true;
        }
      }
      this.applySidebarUnread(data.folders || []);
      this.setUnreadFavicon(total);
      this.setUnreadTitle(total);
      if (data.notifications && typeof Notification !== 'undefined') {
        if (Notification.permission === 'default' && !this._notifAsked) {
          this._notifAsked = true;
          Notification.requestPermission().catch(() => {});
        }
        if (
          this._lastUnreadTotal !== null &&
          total > this._lastUnreadTotal &&
          Notification.permission === 'granted'
        ) {
          const delta = total - this._lastUnreadTotal;
          const first = (data.folders && data.folders[0]) || null;
          new Notification('HimaWeb', {
            body: first
              ? `${delta} nouveau(x) — ${first.label} (${first.unread})`
              : `${delta} nouveau(x) message(s) non lu(s)`,
            tag: 'himaweb-unread',
          });
        }
      }
      this._lastUnreadTotal = total;
    } catch (_) {}
  },

  applySidebarUnread(folders) {
    const nav = document.getElementById('folder-nav');
    if (!nav) return;
    const map = new Map();
    (folders || []).forEach((f) => {
      if (f.key) map.set(f.key, f.unread || 0);
      if (f.mailbox) {
        const k2 = f.account ? `${f.account}::${f.mailbox}` : f.mailbox;
        map.set(k2, f.unread || 0);
        map.set(f.mailbox, (map.get(f.mailbox) || 0) + (f.unread || 0));
      }
    });
    nav.querySelectorAll('[data-unread-for]').forEach((el) => {
      const key = el.getAttribute('data-unread-for');
      const n = map.get(key) || 0;
      if (n > 0) {
        el.hidden = false;
        el.textContent = String(n);
      } else {
        el.remove();
      }
    });
    // Ajouter badges manquants sur les dossiers surveillés
    map.forEach((n, key) => {
      if (n <= 0) return;
      if (nav.querySelector(`[data-unread-for="${cssEsc(key)}"]`)) return;
      const link =
        nav.querySelector(`[data-folder-key="${cssEsc(key)}"]`) ||
        nav.querySelector(`.folder-link[data-folder-key="${cssEsc(key)}"]`);
      const host = link
        ? link.closest('.folder-item') || link
        : null;
      if (!host) return;
      const span = document.createElement('span');
      span.className = 'unread';
      span.setAttribute('data-unread-for', key);
      span.textContent = String(n);
      host.appendChild(span);
    });
  },

  setUnreadFavicon(n) {
    let link = document.querySelector('link[rel="icon"][data-himaweb]');
    if (!link) {
      link = document.createElement('link');
      link.rel = 'icon';
      link.setAttribute('data-himaweb', '1');
      document.head.appendChild(link);
    }
    const size = 32;
    const canvas = document.createElement('canvas');
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    // Montagne simple
    ctx.fillStyle = '#2f6fed';
    ctx.beginPath();
    ctx.moveTo(4, 26);
    ctx.lineTo(16, 6);
    ctx.lineTo(28, 26);
    ctx.closePath();
    ctx.fill();
    ctx.fillStyle = '#fff';
    ctx.beginPath();
    ctx.moveTo(12, 16);
    ctx.lineTo(16, 10);
    ctx.lineTo(20, 16);
    ctx.closePath();
    ctx.fill();
    if (n > 0) {
      const label = n > 99 ? '99+' : String(n);
      ctx.fillStyle = '#e11d48';
      ctx.beginPath();
      ctx.arc(24, 8, 9, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = '#fff';
      ctx.font = 'bold 10px sans-serif';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText(label, 24, 8.5);
    }
    link.href = canvas.toDataURL('image/png');
  },

  setUnreadTitle(n) {
    const base = 'HimaWeb';
    document.title = n > 0 ? `(${n > 99 ? '99+' : n}) ${base}` : base;
  },

  startUnreadPolling() {
    const tick = () => {
      const hidden = document.hidden;
      const delay = hidden ? 60000 : 20000;
      this.pollUnread().finally(() => {
        clearTimeout(this._pollTimer);
        this._pollTimer = setTimeout(tick, delay);
      });
    };
    tick();
    document.addEventListener('visibilitychange', () => {
      if (!document.hidden) this.pollUnread();
    });
  },

  reloadEnvelopes() {
    const mb =
      (document.getElementById('current-mailbox') &&
        document.getElementById('current-mailbox').value) ||
      'Inbox';
    const ac =
      (document.getElementById('current-account') &&
        document.getElementById('current-account').value) ||
      '';
    const sortEl = document.getElementById('mail-sort');
    const sort = (sortEl && sortEl.value) || 'date_desc';
    let url =
      '/partials/envelopes?mailbox=' +
      encodeURIComponent(mb) +
      '&sort=' +
      encodeURIComponent(sort) +
      '&page=1';
    if (ac) url += '&account=' + encodeURIComponent(ac);
    if (window.htmx) {
      window.htmx.ajax('GET', url, { target: '#envelope-list', swap: 'innerHTML' });
    }
  },

  onMessageMoved({ id, mailbox, account, to }) {
    const acc = account || '';
    const env = document.querySelector(
      `.envelope[data-id="${CSS.escape(String(id))}"]${acc ? `[data-account="${CSS.escape(acc)}"]` : ''}`
    );
    const messageId = (env && env.dataset.messageId) || '';
    if (messageId && to && mailbox) {
      this.pushMailUndo({
        type: 'move',
        items: [
          {
            account: acc,
            messageId,
            fromMailbox: mailbox,
            toMailbox: to,
            permanent: false,
          },
        ],
      });
    }
    this.onMessagesDeleted({
      items: [{ id, mailbox, account: acc }],
      last: { id, mailbox, account: acc },
    });
  },

  bindMailDragDrop() {
    if (this._dndBound) return;
    this._dndBound = true;
    this._dndPayload = null;

    const clearDropTargets = () => {
      document.querySelectorAll('.drop-target').forEach((el) => el.classList.remove('drop-target'));
    };

    const folderTargetFromEvent = (ev) => {
      const link = ev.target.closest(
        'a.folder-link-text[data-folder-key], a.folder-link[data-folder-key]'
      );
      if (!link) return null;
      if (link.classList.contains('folder-merged')) return null;
      const mailbox = (link.getAttribute('data-folder-key') || '').includes('::')
        ? (link.getAttribute('data-folder-key') || '').split('::').slice(1).join('::')
        : link.getAttribute('data-folder-key') || link.getAttribute('data-label') || '';
      const account = link.getAttribute('data-account') || '';
      if (!mailbox) return null;
      return { el: link.closest('.folder-item') || link, link, mailbox, account };
    };

    document.addEventListener('dragstart', (ev) => {
      const env = ev.target.closest && ev.target.closest('.envelope');
      if (!env || !ev.dataTransfer) return;
      const mbDefault =
        (document.getElementById('current-mailbox') &&
          document.getElementById('current-mailbox').value) ||
        'Inbox';
      const item = {
        id: env.dataset.id,
        account: env.dataset.account || '',
        mailbox: env.dataset.mailbox || mbDefault,
        messageId: env.dataset.messageId || '',
      };
      let items = [item];
      const selected = this.selectedMailItems();
      if (selected.length > 1) {
        const key = `${item.account}\0${item.id}`;
        if (selected.some((s) => `${s.account || ''}\0${s.id}` === key)) {
          items = selected;
        }
      }
      this._dndPayload = items;
      env.classList.add('dragging');
      ev.dataTransfer.effectAllowed = 'move';
      ev.dataTransfer.setData('application/x-himaweb-mail', JSON.stringify(items));
      ev.dataTransfer.setData('text/plain', items.map((i) => i.id).join(','));
    });

    document.addEventListener('dragend', () => {
      document.querySelectorAll('.envelope.dragging').forEach((el) => el.classList.remove('dragging'));
      clearDropTargets();
      this._dndPayload = null;
    });

    document.addEventListener('dragover', (ev) => {
      if (!this._dndPayload) return;
      const target = folderTargetFromEvent(ev);
      if (!target) return;
      ev.preventDefault();
      ev.dataTransfer.dropEffect = 'move';
      clearDropTargets();
      target.el.classList.add('drop-target');
      if (target.link !== target.el) target.link.classList.add('drop-target');
    });

    document.addEventListener('dragleave', (ev) => {
      const el = ev.target.closest && ev.target.closest('.drop-target');
      if (el && !el.contains(ev.relatedTarget)) el.classList.remove('drop-target');
    });

    document.addEventListener('drop', (ev) => {
      const target = folderTargetFromEvent(ev);
      if (!target || !this._dndPayload) return;
      ev.preventDefault();
      clearDropTargets();
      const items = this._dndPayload;
      this._dndPayload = null;
      document.querySelectorAll('.envelope.dragging').forEach((el) => el.classList.remove('dragging'));
      // Refuse if all items already in that folder+account
      const same = items.every(
        (it) =>
          (it.mailbox || '') === target.mailbox &&
          (it.account || '') === (target.account || '')
      );
      if (same) return;
      this.moveMailsToFolder(items, target.mailbox, target.account);
    });
  },

  async moveMailsToFolder(items, toMailbox, toAccount) {
    if (!items || !items.length || !toMailbox) return;
    this.setEnvelopesPending(items, true);
    HWProgress.begin();
    try {
      const res = await fetch('/api/mail/move', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
        body: JSON.stringify({
          items: items.map((it) => ({
            id: it.id,
            mailbox: it.mailbox,
            account: it.account || null,
            message_id: it.messageId || null,
          })),
          to_mailbox: toMailbox,
          to_account: toAccount || null,
        }),
      });
      const data = await res.json();
      const moved = (data && data.moved) || [];
      if (moved.length) {
        this.pushMailUndo({
          type: 'move',
          items: moved.map((m) => ({
            account: m.account || '',
            messageId: m.message_id || '',
            fromMailbox: m.mailbox,
            toMailbox: m.to_mailbox || toMailbox,
            permanent: false,
          })),
        });
        this.onMessagesDeleted({
          items: moved.map((m) => ({
            id: m.id,
            mailbox: m.mailbox,
            account: m.account || '',
          })),
          last: moved[moved.length - 1],
        });
      }
      if (data && data.errors && data.errors.length) {
        console.warn('move errors', data.errors);
        alert(data.errors.join('\n'));
      }
    } catch (e) {
      console.error(e);
      alert('Échec du déplacement');
    } finally {
      HWProgress.end();
      this.setEnvelopesPending(items, false);
    }
  },

  onMessageDeleted({ id, mailbox, account }) {
    this.onMessagesDeleted({
      items: [{ id, mailbox, account: account || '' }],
      last: { id, mailbox, account: account || '' },
    });
  },

  onMessagesDeleted({ items, last }) {
    if (this._handlingMailEvent) return;
    this._handlingMailEvent = true;
    try {
      const listEl = document.getElementById('envelope-list');
      const envs = listEl ? [...listEl.querySelectorAll('.envelope')] : [];
      const removed = new Set(
        (items || []).map((it) => `${it.account || ''}\0${it.id}`)
      );
      let nextGet = null;
      let nextId = null;
      let nextAcc = '';

      const lastId = last && last.id;
      const lastAcc = (last && last.account) || '';
      const lastIdx = envs.findIndex(
        (e) => e.dataset.id === String(lastId) && (e.dataset.account || '') === lastAcc
      );
      if (lastIdx >= 0) {
        for (const neighbor of [envs[lastIdx + 1], envs[lastIdx - 1]]) {
          if (!neighbor) continue;
          const key = `${neighbor.dataset.account || ''}\0${neighbor.dataset.id}`;
          if (!removed.has(key)) {
            nextGet = neighbor.getAttribute('hx-get');
            nextId = neighbor.dataset.id || null;
            nextAcc = neighbor.dataset.account || '';
            break;
          }
        }
      }

      envs.forEach((el) => {
        const key = `${el.dataset.account || ''}\0${el.dataset.id}`;
        if (removed.has(key)) el.remove();
      });
      this.clearMailSelection(true);

      if (nextGet && window.htmx) {
        window.htmx.ajax('GET', nextGet, { target: '#message-pane', swap: 'innerHTML' });
      } else {
        const pane = document.getElementById('message-pane');
        if (pane) {
          pane.innerHTML =
            '<div class="empty-read"><i data-lucide="mail-open"></i><p>Sélectionnez un message</p></div>';
          if (window.lucide) lucide.createIcons();
        }
      }

      const mb =
        (last && last.mailbox) ||
        (document.getElementById('current-mailbox') &&
          document.getElementById('current-mailbox').value) ||
        'Inbox';
      const ac =
        lastAcc ||
        (document.getElementById('current-account') &&
          document.getElementById('current-account').value) ||
        '';
      const sortEl = document.getElementById('mail-sort');
      const sort = (sortEl && sortEl.value) || 'date_desc';
      let url =
        '/partials/envelopes?mailbox=' +
        encodeURIComponent(mb) +
        '&sort=' +
        encodeURIComponent(sort) +
        '&page=1';
      if (ac) url += '&account=' + encodeURIComponent(ac);

      if (window.htmx && listEl) {
        const handler = (ev) => {
          if (!ev.detail || ev.detail.target !== listEl) return;
          document.removeEventListener('htmx:afterSwap', handler);
          if (nextId) {
            const match = [...listEl.querySelectorAll('.envelope')].find(
              (e) => e.dataset.id === String(nextId) && (e.dataset.account || '') === nextAcc
            );
            if (match) {
              listEl.querySelectorAll('.envelope').forEach((e) => e.classList.remove('is-selected'));
              match.classList.add('is-selected');
            }
          }
          if (window.lucide) lucide.createIcons();
        };
        document.addEventListener('htmx:afterSwap', handler);
        window.htmx.ajax('GET', url, { target: '#envelope-list', swap: 'innerHTML' });
      }
      this.reloadSidebar();
      this.pollUnread();
    } finally {
      setTimeout(() => {
        this._handlingMailEvent = false;
      }, 800);
    }
  },

  clearMailSelection(updateBar = true) {
    this._mailSelected = new Set();
    this._mailAnchor = null;
    document.querySelectorAll('.envelope.is-selected, .envelope.is-checked').forEach((e) => {
      e.classList.remove('is-selected', 'is-checked');
    });
    if (updateBar) this.updateSelectionBar();
  },

  updateSelectionBar() {
    let bar = document.getElementById('mail-selection-bar');
    const n = (this._mailSelected && this._mailSelected.size) || 0;
    if (n <= 1) {
      if (bar) bar.remove();
      return;
    }
    if (!bar) {
      const listPane = document.querySelector('.list-pane');
      if (!listPane) return;
      bar = document.createElement('div');
      bar.id = 'mail-selection-bar';
      bar.className = 'mail-selection-bar';
      listPane.appendChild(bar);
    }
    bar.innerHTML = `<span>${n} sélectionnés</span>
      <button type="button" class="btn danger sm" id="mail-sel-delete">
        <i data-lucide="trash-2"></i> Supprimer
      </button>
      <button type="button" class="icon-btn" id="mail-sel-clear" title="Annuler la sélection">
        <i data-lucide="x"></i>
      </button>`;
    if (window.lucide) lucide.createIcons();
    const del = document.getElementById('mail-sel-delete');
    const clr = document.getElementById('mail-sel-clear');
    if (del) del.onclick = () => this.deleteSelectedMails();
    if (clr) clr.onclick = () => this.clearMailSelection();
  },

  selectedMailItems() {
    const list = [
      ...document.querySelectorAll('#envelope-list .envelope, #search-results .envelope'),
    ];
    const mbDefault =
      (document.getElementById('current-mailbox') &&
        document.getElementById('current-mailbox').value) ||
      'Inbox';
    return list
      .filter((el) => el.classList.contains('is-checked') || el.classList.contains('is-selected'))
      .filter((el) => {
        if (this._mailSelected && this._mailSelected.size > 0) {
          const key = `${el.dataset.account || ''}\0${el.dataset.id}`;
          return this._mailSelected.has(key);
        }
        return el.classList.contains('is-selected');
      })
      .map((el) => ({
        id: el.dataset.id,
        account: el.dataset.account || '',
        mailbox: el.dataset.mailbox || mbDefault,
        messageId: el.dataset.messageId || '',
      }));
  },

  async deleteSelectedMails() {
    const items = this.selectedMailItems();
    if (!items.length) return;
    const label =
      items.length === 1
        ? 'Supprimer ce message ?'
        : `Supprimer ${items.length} messages ?`;
    const ok = await this.confirmDelete(label);
    if (!ok) return;
    this.deleteMailsApi(items);
  },

  async deleteMailsApi(items) {
    if (!items || !items.length) return;
    this.setEnvelopesPending(items, true);
    HWProgress.begin();
    try {
      const res = await fetch('/api/mail/delete', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
        body: JSON.stringify({
          items: items.map((it) => ({
            id: it.id,
            mailbox: it.mailbox,
            account: it.account || null,
            message_id: it.messageId || null,
          })),
        }),
      });
      const data = await res.json();
      const deleted = (data && data.deleted) || [];
      if (deleted.length) {
        this.pushMailUndo({
          type: 'delete',
          items: deleted.map((m) => ({
            account: m.account || '',
            messageId: m.message_id || '',
            fromMailbox: m.mailbox,
            toMailbox: m.to_mailbox || '',
            permanent: !!m.permanent,
          })),
        });
        this.onMessagesDeleted({
          items: deleted.map((m) => ({
            id: m.id,
            mailbox: m.mailbox,
            account: m.account || '',
          })),
          last: deleted[deleted.length - 1],
        });
      } else {
        this.clearMailSelection(true);
      }
      if (data && data.errors && data.errors.length) {
        console.warn('delete errors', data.errors);
        alert(data.errors.join('\n'));
      }
    } catch (e) {
      console.error(e);
      alert('Échec de la suppression');
    } finally {
      HWProgress.end();
      // Les enveloppes supprimées ont disparu ; restaurer celles qui restent.
      this.setEnvelopesPending(items, false);
    }
  },

  bindMailKeys() {
    if (this._mailKeysBound) return;
    this._mailKeysBound = true;
    this._mailSelected = new Set();
    this._mailAnchor = null;

    const isTyping = (el) => {
      if (!el) return false;
      const tag = (el.tagName || '').toLowerCase();
      if (tag === 'input' || tag === 'textarea' || tag === 'select') return true;
      if (el.isContentEditable) return true;
      return !!el.closest('[contenteditable="true"]');
    };

    const envelopes = () =>
      [...document.querySelectorAll('#envelope-list .envelope, #search-results .envelope')];

    const envKey = (el) => `${el.dataset.account || ''}\0${el.dataset.id}`;

    const selectedIdx = (list) => {
      const i = list.findIndex((el) => el.classList.contains('is-selected'));
      if (i >= 0) return i;
      return list.findIndex((el) => el.classList.contains('active') || el.matches(':focus'));
    };

    const paintSelection = (list) => {
      list.forEach((el) => {
        const on = this._mailSelected.has(envKey(el));
        el.classList.toggle('is-checked', on);
        el.classList.toggle('is-selected', on);
      });
      this.updateSelectionBar();
    };

    const selectSingle = (el, { open } = { open: true }) => {
      const list = envelopes();
      this._mailSelected = new Set([envKey(el)]);
      this._mailAnchor = list.indexOf(el);
      paintSelection(list);
      el.focus({ preventScroll: false });
      el.scrollIntoView({ block: 'nearest' });
      if (open && window.htmx) {
        window.htmx.trigger(el, 'click');
      }
    };

    const selectRange = (toEl) => {
      const list = envelopes();
      const to = list.indexOf(toEl);
      if (to < 0) return;
      let from = this._mailAnchor;
      if (from == null || from < 0) from = to;
      const a = Math.min(from, to);
      const b = Math.max(from, to);
      this._mailSelected = new Set();
      for (let i = a; i <= b; i++) this._mailSelected.add(envKey(list[i]));
      paintSelection(list);
      toEl.focus({ preventScroll: false });
      toEl.scrollIntoView({ block: 'nearest' });
    };

    const toggleOne = (el) => {
      const list = envelopes();
      const key = envKey(el);
      if (this._mailSelected.has(key)) this._mailSelected.delete(key);
      else this._mailSelected.add(key);
      this._mailAnchor = list.indexOf(el);
      paintSelection(list);
      el.focus({ preventScroll: false });
    };

    const deleteCurrent = () => {
      if (this._mailSelected && this._mailSelected.size > 1) {
        this.deleteSelectedMails();
        return;
      }
      if (this._mailSelected && this._mailSelected.size === 1) {
        this.deleteSelectedMails();
        return;
      }
      const form = document.querySelector('#message-pane form[hx-post="/partials/message/delete"]');
      if (form) {
        form.requestSubmit();
        return;
      }
    };

    const openAction = (sel) => {
      const a = document.querySelector(`#message-pane ${sel}`);
      if (a) a.click();
    };

    document.addEventListener('keydown', (ev) => {
      if (isTyping(ev.target)) return;

      const list = envelopes();
      const key = ev.key;

      // Shift+↑/↓ : étendre la sélection
      if ((key === 'ArrowDown' || key === 'ArrowUp' || key === 'j' || key === 'k') && ev.shiftKey) {
        if (!list.length) return;
        ev.preventDefault();
        const i = selectedIdx(list);
        const delta = key === 'ArrowDown' || key === 'j' ? 1 : -1;
        const base = i < 0 ? 0 : i;
        const next = list[Math.min(list.length - 1, Math.max(0, base + delta))];
        if (this._mailAnchor == null) this._mailAnchor = base;
        selectRange(next);
        return;
      }

      if ((ev.ctrlKey || ev.metaKey) && !ev.altKey) {
        if (key === 'a' || key === 'A') {
          if (!list.length) return;
          ev.preventDefault();
          this._mailSelected = new Set(list.map((el) => envKey(el)));
          this._mailAnchor = 0;
          paintSelection(list);
          return;
        }
        if (key === 'z' || key === 'Z') {
          if (!this._mailUndoStack || !this._mailUndoStack.length) return;
          ev.preventDefault();
          this.undoLastMailOp();
          return;
        }
        return;
      }

      if (ev.altKey) return;

      if (key === 'ArrowDown' || key === 'j') {
        if (!list.length) return;
        ev.preventDefault();
        const i = selectedIdx(list);
        const next = list[Math.min(list.length - 1, Math.max(0, i) + (i < 0 ? 0 : 1))];
        selectSingle(next);
        if (next === list[list.length - 1]) {
          const more = document.querySelector('#envelope-list .hw-load-more');
          if (more && !more.classList.contains('htmx-request') && !more.disabled) {
            more.click();
          }
        }
        return;
      }
      if (key === 'ArrowUp' || key === 'k') {
        if (!list.length) return;
        ev.preventDefault();
        const i = selectedIdx(list);
        const prev = list[Math.max(0, (i < 0 ? 0 : i) - 1)];
        selectSingle(prev);
        return;
      }
      if (key === 'Enter') {
        const i = selectedIdx(list);
        if (i < 0) return;
        ev.preventDefault();
        selectSingle(list[i], { open: true });
        return;
      }
      if (key === 'Delete' || key === 'Backspace') {
        if (
          !(this._mailSelected && this._mailSelected.size > 0) &&
          !document.querySelector('#message-pane .message-view, #message-pane .thread-view')
        ) {
          return;
        }
        ev.preventDefault();
        deleteCurrent();
        return;
      }
      if (key === 'r' || key === 'R') {
        if (!document.querySelector('#message-pane .message-view, #message-pane .thread-view'))
          return;
        ev.preventDefault();
        openAction('a[title="Répondre"]');
        return;
      }
      if (key === 'f' || key === 'F') {
        if (!document.querySelector('#message-pane .message-view, #message-pane .thread-view'))
          return;
        ev.preventDefault();
        openAction('a[title="Transférer"]');
        return;
      }
      if (key === 'u' || key === 'U') {
        if (!document.querySelector('#message-pane .message-view, #message-pane .thread-view'))
          return;
        ev.preventDefault();
        const btn = document.querySelector(
          '#message-pane form[hx-post="/partials/message/flag"] button[type="submit"]'
        );
        if (btn) btn.click();
        return;
      }
      if (key === 'Escape') {
        if (this._mailSelected && this._mailSelected.size > 1) {
          ev.preventDefault();
          this.clearMailSelection();
          return;
        }
        const pane = document.getElementById('message-pane');
        if (pane && pane.querySelector('.message-view, .thread-view')) {
          ev.preventDefault();
          pane.innerHTML =
            '<div class="empty-read"><i data-lucide="mail-open"></i><p>Sélectionnez un message</p></div>';
          if (window.lucide) lucide.createIcons();
        }
      }
    });

    // Clic : Maj = plage, Ctrl = bascule, sinon mono + ouverture HTMX
    document.addEventListener(
      'click',
      (ev) => {
        const env = ev.target.closest('.envelope');
        if (!env) return;
        if (ev.shiftKey) {
          ev.preventDefault();
          ev.stopPropagation();
          selectRange(env);
          return;
        }
        if (ev.ctrlKey || ev.metaKey) {
          ev.preventDefault();
          ev.stopPropagation();
          toggleOne(env);
          return;
        }
        // mono — laisser HTMX ouvrir, juste peindre la sélection
        const list = envelopes();
        this._mailSelected = new Set([envKey(env)]);
        this._mailAnchor = list.indexOf(env);
        paintSelection(list);
        // Retour immédiat : l'ouverture d'un message passe par un appel CLI.
        if (env.hasAttribute('hx-get')) {
          this.markEnvelopeOpening(env);
          this.showMessageLoading();
        }
      },
      true
    );
  },

  toggleSideWidget(force) {
    const w = document.getElementById('side-widget');
    if (!w) return;
    const open = force != null ? !!force : w.getAttribute('data-open') !== '1';
    w.setAttribute('data-open', open ? '1' : '0');
    try {
      localStorage.setItem('himaweb-side-open', open ? '1' : '0');
    } catch (_) {}
    if (open && window.htmx) {
      const body = w.querySelector('.side-widget-body');
      if (body) window.htmx.ajax('GET', '/partials/side-widget', { target: body, swap: 'innerHTML' });
    }
    if (window.lucide) lucide.createIcons();
  },

  sideContactSearch(q) {
    clearTimeout(this._sideContactTimer);
    this._sideContactTimer = setTimeout(async () => {
      const list = document.getElementById('side-contact-results');
      if (!list) return;
      const query = (q || '').trim();
      if (query.length < 2) {
        list.innerHTML = '';
        return;
      }
      try {
        const res = await fetch('/api/contacts/suggest?q=' + encodeURIComponent(query));
        const data = await res.json();
        const items = data.items || [];
        list.innerHTML = items
          .slice(0, 8)
          .map((it) => {
            const email = (it.email || '').replace(/"/g, '&quot;');
            const label = (it.label || it.name || email).replace(/</g, '&lt;');
            return `<li><a href="/compose?to=${encodeURIComponent(email)}">${label}</a><span class="muted tiny">${email}</span></li>`;
          })
          .join('');
      } catch (_) {
        list.innerHTML = '';
      }
    }, 200);
  },

  bindContextMenus() {
    if (this._ctxBound) return;
    this._ctxBound = true;
    let menu = document.getElementById('ctx-menu');
    if (!menu) {
      menu = document.createElement('div');
      menu.id = 'ctx-menu';
      menu.className = 'ctx-menu';
      document.body.appendChild(menu);
    }
    const hide = () => menu.classList.remove('open');
    document.addEventListener('click', hide);
    document.addEventListener('scroll', hide, true);
    document.addEventListener('keydown', (ev) => {
      if (ev.key === 'Escape') hide();
    });

    const show = (x, y, html) => {
      menu.innerHTML = html;
      menu.classList.add('open');
      const rect = menu.getBoundingClientRect();
      let left = x;
      let top = y;
      if (left + rect.width > window.innerWidth - 8) left = window.innerWidth - rect.width - 8;
      if (top + rect.height > window.innerHeight - 8) top = window.innerHeight - rect.height - 8;
      menu.style.left = Math.max(8, left) + 'px';
      menu.style.top = Math.max(8, top) + 'px';
      if (window.lucide) lucide.createIcons();
      menu.querySelectorAll('[data-ctx-action]').forEach((btn) => {
        btn.addEventListener('click', (ev) => {
          ev.preventDefault();
          const action = btn.getAttribute('data-ctx-action');
          const payload = JSON.parse(btn.getAttribute('data-ctx-payload') || '{}');
          hide();
          this.runContextAction(action, payload);
        });
      });
    };

    // Quick actions au survol (délégation)
    document.addEventListener(
      'click',
      (ev) => {
        const btn = ev.target.closest('[data-qa]');
        if (!btn || !btn.closest('[data-env-quick]')) return;
        const env = btn.closest('.envelope');
        if (!env) return;
        ev.preventDefault();
        ev.stopPropagation();
        const id = env.dataset.id || '';
        const account = env.dataset.account || '';
        const mailbox =
          env.dataset.mailbox ||
          (document.getElementById('current-mailbox') &&
            document.getElementById('current-mailbox').value) ||
          'Inbox';
        const threadIds = (env.dataset.threadIds || id)
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean);
        const messageId = env.dataset.messageId || '';
        const unread = env.classList.contains('unread');
        const qa = btn.getAttribute('data-qa');
        if (qa === 'flag') {
          this.runContextAction('flag', {
            id,
            account,
            mailbox,
            seen: unread ? '1' : '0',
          });
        } else if (qa === 'archive' || qa === 'spam' || qa === 'delete') {
          this.runContextAction(qa, { id, account, mailbox, threadIds, messageId });
        }
      },
      true
    );

    document.addEventListener('contextmenu', (ev) => {
      const env = ev.target.closest('.envelope');
      if (env) {
        ev.preventDefault();
        const id = env.dataset.id || '';
        const account = env.dataset.account || '';
        const mailbox =
          env.dataset.mailbox ||
          (document.getElementById('current-mailbox') &&
            document.getElementById('current-mailbox').value) ||
          'Inbox';
        const unread = env.classList.contains('unread');
        const threadIds = (env.dataset.threadIds || id)
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean);
        const messageId = env.dataset.messageId || '';
        const payload = JSON.stringify({ id, account, mailbox, threadIds, messageId });
        const delLabel =
          threadIds.length > 1
            ? `Supprimer la conversation (${threadIds.length})`
            : 'Supprimer';
        const junk = this.isJunkMailbox(mailbox);
        const spamLabel = junk ? 'Pas du spam' : 'Signaler spam';
        const spamIcon = junk ? 'shield-check' : 'shield-alert';
        show(
          ev.clientX,
          ev.clientY,
          `<button type="button" data-ctx-action="open" data-ctx-payload='${payload}'><i data-lucide="mail-open"></i> Ouvrir</button>
           <button type="button" data-ctx-action="flag" data-ctx-payload='${JSON.stringify({
             id,
             account,
             mailbox,
             seen: unread ? '1' : '0',
           })}'><i data-lucide="${unread ? 'mail-open' : 'mail'}"></i> ${
             unread ? 'Marquer lu' : 'Marquer non lu'
           }</button>
           <button type="button" data-ctx-action="reply" data-ctx-payload='${payload}'><i data-lucide="reply"></i> Répondre</button>
           <button type="button" data-ctx-action="forward" data-ctx-payload='${payload}'><i data-lucide="forward"></i> Transférer</button>
           <hr/>
           <button type="button" data-ctx-action="archive" data-ctx-payload='${payload}'><i data-lucide="archive"></i> Archiver</button>
           <button type="button" data-ctx-action="spam" data-ctx-payload='${payload}'><i data-lucide="${spamIcon}"></i> ${spamLabel}</button>
           <button type="button" class="danger" data-ctx-action="delete" data-ctx-payload='${payload}'><i data-lucide="trash-2"></i> ${delLabel}</button>`
        );
        return;
      }
      const cal = ev.target.closest('.cal-ev-click');
      if (cal) {
        ev.preventDefault();
        let payload = { id: '', calendar: '', summary: '' };
        try {
          const raw = cal.getAttribute('data-ev');
          if (raw) {
            const obj = JSON.parse(raw);
            payload = {
              id: obj.id || '',
              calendar: obj.calendar || obj.calendar_id || '',
              summary: obj.summary || obj.title || '',
            };
          }
        } catch (_) {}
        const pl = JSON.stringify(payload).replace(/'/g, '&#39;');
        show(
          ev.clientX,
          ev.clientY,
          `<button type="button" data-ctx-action="cal-edit" data-ctx-payload='${pl}'><i data-lucide="pencil"></i> Modifier</button>
           <button type="button" class="danger" data-ctx-action="cal-delete" data-ctx-payload='${pl}'><i data-lucide="trash-2"></i> Supprimer</button>`
        );
        return;
      }
      const contact = ev.target.closest('.contact-row');
      if (contact) {
        const email = contact.dataset.email || '';
        const id = contact.dataset.id || '';
        const book = contact.dataset.bookRef || contact.dataset.book || '';
        if (!email && !id) return;
        ev.preventDefault();
        const payload = JSON.stringify({ email, id, book });
        show(
          ev.clientX,
          ev.clientY,
          `<button type="button" data-ctx-action="contact-mail" data-ctx-payload='${payload}'><i data-lucide="pen-square"></i> Écrire</button>
           <button type="button" class="danger" data-ctx-action="contact-delete" data-ctx-payload='${payload}'><i data-lucide="trash-2"></i> Supprimer</button>`
        );
      }
    });
  },

  isJunkMailbox(name) {
    const n = String(name || '').toLowerCase();
    return n.includes('junk') || n.includes('spam');
  },

  isArchiveMailbox(name) {
    const n = String(name || '').toLowerCase();
    return n.includes('archive') && !n.includes('inbox');
  },

  /** Résout Archive / Junk / Inbox depuis la sidebar (fallback si dossier absent). */
  resolveSpecialFolder(kind, account) {
    const acc = (account || '').trim();
    const names = [];
    const nav = document.getElementById('folder-nav');
    if (nav) {
      nav.querySelectorAll('[data-folder-key]').forEach((el) => {
        const elAcc = (el.dataset.account || '').trim();
        if (acc && elAcc && elAcc !== acc) return;
        let key = el.dataset.folderKey || '';
        if (elAcc && key.startsWith(elAcc + '::')) key = key.slice(elAcc.length + 2);
        if (key) names.push(key);
      });
    }
    const find = (preds, fallback) => {
      for (const pred of preds) {
        const hit = names.find((n) => pred(n.toLowerCase()));
        if (hit) return hit;
      }
      return fallback;
    };
    if (kind === 'inbox') {
      return find([(n) => n === 'inbox' || n.endsWith('/inbox')], 'Inbox');
    }
    if (kind === 'archive') {
      return find(
        [
          (n) => n === 'archive' || n.endsWith('/archive'),
          (n) => n.includes('archive') && !n.includes('inbox'),
        ],
        'Archive'
      );
    }
    if (kind === 'junk') {
      return find(
        [
          (n) => n === 'junk' || n === 'spam' || n.endsWith('/junk') || n.endsWith('/spam'),
          (n) => n.includes('junk') || n.includes('spam'),
        ],
        'Junk'
      );
    }
    return kind;
  },

  envelopeItemsFromPayload(p) {
    const mb = p.mailbox || 'Inbox';
    const acc = p.account || '';
    const id = p.id || '';
    const threadIds = Array.isArray(p.threadIds)
      ? p.threadIds.map(String).filter(Boolean)
      : id
        ? [id]
        : [];
    const ids = threadIds.length ? threadIds : id ? [id] : [];
    return ids.map((tid) => {
      const el =
        document.querySelector(
          `.envelope[data-id="${cssEsc(tid)}"][data-account="${cssEsc(acc)}"]`
        ) || document.querySelector(`.envelope[data-id="${cssEsc(tid)}"]`);
      return {
        id: tid,
        mailbox: mb,
        account: acc,
        messageId: (el && el.dataset.messageId) || p.messageId || '',
      };
    });
  },

  runContextAction(action, p) {
    const mb = p.mailbox || 'Inbox';
    const acc = p.account || '';
    const id = p.id || '';
    if (action === 'open') {
      let url =
        '/partials/message?mailbox=' +
        encodeURIComponent(mb) +
        '&id=' +
        encodeURIComponent(id);
      if (acc) url += '&account=' + encodeURIComponent(acc);
      if (window.htmx) window.htmx.ajax('GET', url, { target: '#message-pane', swap: 'innerHTML' });
      return;
    }
    if (action === 'reply') {
      this.openCompose(
        '/compose?kind=reply&mailbox=' +
          encodeURIComponent(mb) +
          '&id=' +
          encodeURIComponent(id) +
          (acc ? '&account=' + encodeURIComponent(acc) : '')
      );
      return;
    }
    if (action === 'forward') {
      this.openCompose(
        '/compose?kind=forward&mailbox=' +
          encodeURIComponent(mb) +
          '&id=' +
          encodeURIComponent(id) +
          (acc ? '&account=' + encodeURIComponent(acc) : '')
      );
      return;
    }
    if (action === 'flag') {
      const body = new URLSearchParams();
      body.set('mailbox', mb);
      body.set('id', id);
      body.set('account', acc);
      body.set('seen', p.seen || '1');
      body.set('quiet', '1');
      if (window.htmx) {
        window.htmx.ajax('POST', '/partials/message/flag', {
          target: '#htmx-sink',
          swap: 'innerHTML',
          values: Object.fromEntries(body),
        });
      }
      // Feedback immédiat sur la ligne
      const env = document.querySelector(
        `.envelope[data-id="${cssEsc(id)}"][data-account="${cssEsc(acc)}"]`
      ) || document.querySelector(`.envelope[data-id="${cssEsc(id)}"]`);
      if (env) {
        const markRead = (p.seen || '1') === '1';
        env.classList.toggle('unread', !markRead);
        const qa = env.querySelector('[data-qa="flag"]');
        if (qa) {
          qa.title = markRead ? 'Marquer non lu' : 'Marquer lu';
          qa.innerHTML = `<i data-lucide="${markRead ? 'mail' : 'mail-open'}"></i>`;
          if (window.lucide) lucide.createIcons();
        }
      }
      return;
    }
    if (action === 'archive') {
      const items = this.envelopeItemsFromPayload(p);
      if (!items.length) return;
      const dest = this.resolveSpecialFolder('archive', acc);
      this.moveMailsToFolder(items, dest, acc);
      return;
    }
    if (action === 'spam') {
      const items = this.envelopeItemsFromPayload(p);
      if (!items.length) return;
      const junk = this.isJunkMailbox(mb);
      const dest = junk
        ? this.resolveSpecialFolder('inbox', acc)
        : this.resolveSpecialFolder('junk', acc);
      this.moveMailsToFolder(items, dest, acc);
      return;
    }
    if (action === 'delete') {
      const threadIds = Array.isArray(p.threadIds)
        ? p.threadIds.map(String).filter(Boolean)
        : id
          ? [id]
          : [];
      const ids = threadIds.length ? threadIds : id ? [id] : [];
      if (!ids.length) return;
      const label =
        ids.length > 1
          ? `Supprimer cette conversation (${ids.length} messages) ?`
          : 'Supprimer ce message ?';
      this.confirmDelete(label).then((ok) => {
        if (!ok) return;
        const items = ids.map((tid) => {
          const el = document.querySelector(
            `.envelope[data-id="${CSS.escape(tid)}"][data-account="${CSS.escape(acc)}"]`
          ) || document.querySelector(`.envelope[data-id="${CSS.escape(tid)}"]`);
          return {
            id: tid,
            mailbox: mb,
            account: acc,
            messageId: (el && el.dataset.messageId) || p.messageId || '',
          };
        });
        this.deleteMailsApi(items);
      });
      return;
    }
    if (action === 'cal-edit') {
      const nodes = document.querySelectorAll('.cal-ev-click[data-ev]');
      for (const btn of nodes) {
        try {
          const obj = JSON.parse(btn.getAttribute('data-ev') || '{}');
          if (String(obj.id || '') === String(p.id || '')) {
            btn.click();
            return;
          }
        } catch (_) {}
      }
      return;
    }
    if (action === 'cal-delete') {
      if (!window.confirm('Supprimer cet événement ?')) return;
      const form = document.createElement('form');
      form.method = 'post';
      form.action = '/calendar/delete';
      form.innerHTML = `<input type="hidden" name="id" value="${String(p.id).replace(
        /"/g,
        '&quot;'
      )}"/><input type="hidden" name="calendar" value="${String(p.calendar || '').replace(
        /"/g,
        '&quot;'
      )}"/>`;
      document.body.appendChild(form);
      form.submit();
      return;
    }
    if (action === 'contact-mail') {
      this.openCompose('/compose?to=' + encodeURIComponent(p.email || ''));
      return;
    }
    if (action === 'contact-delete') {
      if (!window.confirm('Supprimer ce contact ?')) return;
      const form = document.createElement('form');
      form.method = 'post';
      form.action = '/contacts/delete';
      form.innerHTML = `<input type="hidden" name="id" value="${String(p.id || '').replace(
        /"/g,
        '&quot;'
      )}"/><input type="hidden" name="book" value="${String(p.book || '').replace(
        /"/g,
        '&quot;'
      )}"/>`;
      document.body.appendChild(form);
      form.submit();
    }
  },
};

function cssEsc(s) {
  if (window.CSS && CSS.escape) return CSS.escape(s);
  return String(s).replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

document.addEventListener('htmx:afterSwap', (ev) => {
  const t = ev.detail && ev.detail.target;
  // Rendu limité au fragment muté : voir hwRenderIcons.
  hwRenderIcons(t || document);
  if (t && (t.id === 'message-pane' || t.id === 'envelope-list')) {
    window.HimaWeb.setPaneBusy(t, false);
  }
  if (t && t.id === 'message-pane') window.HimaWeb.clearEnvelopeOpening();
  if (t && window.Alpine && typeof Alpine.initTree === 'function') {
    if (
      t.id === 'compose-layer' ||
      t.id === 'message-pane' ||
      t.classList.contains('side-widget-body') ||
      (t.querySelector && t.querySelector('.side-widget-inner, .compose-backdrop, .thread-view'))
    ) {
      Alpine.initTree(t);
    }
  }
  if (t && (t.id === 'sidebar' || (t.querySelector && t.querySelector('#folder-nav')))) {
    window.HimaWeb.bindFolderClicks();
    window.HimaWeb.bindFolderTree(true);
    window.HimaWeb.applyRailCompactFromStorage();
  }
  // Fallback si les <script> du fragment ne s'exécutent pas
  if (t && t.id === 'message-pane' && window.HimaWeb) {
    const evEl = t.querySelector('[data-mail-event]');
    if (evEl && !window.HimaWeb._handlingMailEvent) {
      const kind = evEl.getAttribute('data-mail-event');
      if (kind === 'deleted-batch') {
        let items = [];
        try {
          items = JSON.parse(evEl.getAttribute('data-items') || '[]');
        } catch (_) {}
        window.HimaWeb.onMessagesDeleted({
          items,
          last: {
            id: evEl.getAttribute('data-id') || '',
            mailbox: evEl.getAttribute('data-mailbox') || '',
            account: evEl.getAttribute('data-account') || '',
          },
        });
      } else {
        const payload = {
          id: evEl.getAttribute('data-id') || '',
          mailbox: evEl.getAttribute('data-mailbox') || '',
          account: evEl.getAttribute('data-account') || '',
          to: evEl.getAttribute('data-to') || '',
        };
        if (kind === 'deleted') window.HimaWeb.onMessageDeleted(payload);
        else if (kind === 'moved') window.HimaWeb.onMessageMoved(payload);
      }
    }
    const thread = t.querySelector('.thread-view[data-marked-read]');
    if (thread) {
      window.HimaWeb.markEnvelopeRead(
        thread.getAttribute('data-msg-id') || '',
        thread.getAttribute('data-msg-account') || ''
      );
    }
  }
});

document.addEventListener('click', (ev) => {
  const a = ev.target.closest && ev.target.closest('a[href]');
  if (!a || ev.defaultPrevented || ev.metaKey || ev.ctrlKey || ev.shiftKey || ev.altKey) return;
  if (a.target && a.target !== '_self') return;
  const href = a.getAttribute('href') || '';
  if (!href.startsWith('/compose')) return;
  if (!document.getElementById('compose-layer')) return;
  ev.preventDefault();
  window.HimaWeb.openCompose(href);
}, true);

document.addEventListener('DOMContentLoaded', () => {
  hwRenderIcons(document);
  window.HimaWeb.bindFolderClicks();
  window.HimaWeb.bindFolderTree(true);
  window.HimaWeb.bindColumnResize();
  window.HimaWeb.bindMailKeys();
  window.HimaWeb.bindContextMenus();
  window.HimaWeb.bindMailDragDrop();
  window.HimaWeb.bindDeleteFormConfirm();
  window.HimaWeb.bindFormBusy();
  window.HimaWeb.bindMessagePrefetch();
  window.HimaWeb.bindUiPreview();
  window.HimaWeb.loadPrefs();
  window.HimaWeb.startUnreadPolling();
  window.HimaWeb.applyRailCompactFromStorage();
  try {
    const side = document.getElementById('side-widget');
    if (side && localStorage.getItem('himaweb-side-open') === '0') {
      side.setAttribute('data-open', '0');
    }
  } catch (_) {}
});
