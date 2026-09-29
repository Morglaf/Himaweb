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
    to: '',
    cc: '',
    bcc: '',
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
    windowMode: 'normal',
    quill: null,
    suggestions: { to: [], cc: [], bcc: [] },
    get selectedAccount() {
      return this.accounts.find((a) => a.name === this.account) || this.accounts[0] || null;
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
          this.to = opts.to || '';
          this.cc = opts.cc || '';
          this.bcc = opts.bcc || '';
          this.subject = opts.subject || '';
          this.accounts = opts.accounts || [];
          this.account = opts.account || (this.accounts[0] && this.accounts[0].name) || '';
          this.showCc = !!(this.cc || this.bcc);
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
        });
      } else {
        this.syncPlainFromQuill();
        this.bodyMode = 'plain';
        this.destroyQuill();
      }
      try {
        sessionStorage.setItem('himaweb-compose-body-mode', this.bodyMode);
      } catch (_) {}
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
          if (attempt < 20) {
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
      tryInit(0);
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
        this.aiError = 'Décrivez ce que vous voulez écrire.';
        return;
      }
      this.aiBusy = true;
      this.aiError = '';
      try {
        const res = await fetch('/ai/api/mail', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            prompt,
            kind: this.kind,
            context: [this.subject, this.body].filter(Boolean).join('\n\n'),
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

function eventModal(opts) {
  opts = opts || {};
  const firstCal = opts.defaultCalendar && opts.defaultCalendar !== '__all__'
    ? opts.defaultCalendar
    : '';
  return {
    open: false,
    mode: 'create',
    ai: !!opts.ai,
    aiOpen: false,
    aiPrompt: '',
    aiBusy: false,
    aiError: '',
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
    openCreate() {
      this.mode = 'create';
      this.id = '';
      this.summary = '';
      this.startDate = opts.defaultDate || '';
      this.endDate = opts.defaultDate || '';
      this.startTime = '10:00';
      this.endTime = '11:00';
      this.location = '';
      this.description = '';
      this.rrule = 'none';
      this.aiPrompt = '';
      this.aiError = '';
      if (firstCal) this.calendar = firstCal;
      this.open = true;
      this.$nextTick(() => { if (window.lucide) lucide.createIcons(); });
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
      this.open = true;
      this.$nextTick(() => { if (window.lucide) lucide.createIcons(); });
    },
    close() {
      this.open = false;
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
        const res = await fetch('/ai/api/event', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ prompt }),
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
    const res = await fetch('https://unpkg.com/lucide-static@0.469.0/tags.json');
    if (!res.ok) throw new Error('tags');
    const tags = await res.json();
    window.__lucideIconNames = Object.keys(tags).sort();
  } catch (_) {
    window.__lucideIconNames = LUCIDE_FALLBACK.slice();
  }
  return window.__lucideIconNames;
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

document.addEventListener('alpine:init', () => {
  if (window.Alpine) {
    Alpine.data('composeForm', composeForm);
    Alpine.data('accountAppearance', accountAppearance);
    Alpine.data('quickEventModal', quickEventModal);
    Alpine.data('tbImportForm', tbImportForm);
  }
});


window.HimaWeb = {
  _lastUnreadTotal: null,
  _folderClicksBound: false,
  _folderTreeBound: false,
  _resizeBound: false,

  settingsSaved(form, evt) {
    if (!form) return;
    if (evt && evt.detail && evt.detail.successful === false) return;
    const el = form.querySelector('.settings-saved');
    if (!el) return;
    el.hidden = false;
    clearTimeout(form._savedTimer);
    form._savedTimer = setTimeout(() => {
      el.hidden = true;
    }, 1600);
  },

  markEnvelopeRead(id, account) {
    const acc = account || '';
    let changed = false;
    document.querySelectorAll('.envelope.unread').forEach((el) => {
      if (el.dataset.id !== String(id)) return;
      if ((el.dataset.account || '') !== acc) return;
      el.classList.remove('unread');
      changed = true;
      el.querySelectorAll('.from strong').forEach((s) => {
        const parent = s.parentNode;
        while (s.firstChild) parent.insertBefore(s.firstChild, s);
        s.remove();
      });
    });
    if (changed) this.bumpUnread(-1);
    this.scheduleMailRefresh({ envelopes: false, sidebar: true, unread: true, unreadDelay: 900 });
  },

  applyEnvelopeSeen(id, account, seen) {
    const acc = account || '';
    let changed = false;
    document.querySelectorAll('.envelope').forEach((el) => {
      if (el.dataset.id !== String(id)) return;
      if ((el.dataset.account || '') !== acc) return;
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
    if (changed) this.bumpUnread(seen ? -1 : 1);
    this.scheduleMailRefresh({ envelopes: false, sidebar: true, unread: true, unreadDelay: 900 });
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
        root._x_dataStack[0].closeModal();
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
      if (listTitle && color) listTitle.style.setProperty('--list-title-color', color);
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
        if (window.lucide) lucide.createIcons();
      }
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

    // Replier tout (sauf profondeur 0), puis ouvrir le chemin courant
    nav.querySelectorAll('.folder-row[data-depth]').forEach((row) => {
      const depth = parseInt(row.dataset.depth || '0', 10);
      row.classList.toggle('is-folded', depth > 0);
    });
    nav.querySelectorAll('.folder-twist').forEach((btn) => {
      btn.setAttribute('aria-expanded', 'false');
      btn.setAttribute('aria-label', 'Déplier');
      btn.classList.remove('open');
    });

    const current = nav.getAttribute('data-current') || '';
    const active = nav.querySelector('.folder-item.active');
    const activeRow = active && active.closest('.folder-row');
    if (activeRow) {
      // Remonter les parents
      let parentId = activeRow.dataset.parent || '';
      while (parentId) {
        this.setFolderExpanded(nav, parentId, true);
        const parentRow = nav.querySelector(`.folder-row[data-tree-id="${cssEsc(parentId)}"]`);
        parentId = parentRow ? parentRow.dataset.parent || '' : '';
      }
    } else if (current) {
      // Fallback : déplier Archive si current = Archive/…
      const parts = current.split('/');
      let acc = '';
      for (let i = 0; i < parts.length - 1; i++) {
        acc = acc ? acc + '/' + parts[i] : parts[i];
        const row = [...nav.querySelectorAll('.folder-row')].find((r) => {
          const id = r.dataset.treeId || '';
          return id === acc || id.endsWith('::' + acc);
        });
        if (row) this.setFolderExpanded(nav, row.dataset.treeId, true);
      }
    }

    if (window.lucide) lucide.createIcons();
    void force;
  },

  setFolderExpanded(nav, path, expanded) {
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
          window.HimaWeb.setFolderExpanded(nav, row.dataset.treeId, false);
        }
      }
    });
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

  onMessageMoved({ id, mailbox, account }) {
    this.onMessagesDeleted({
      items: [{ id, mailbox, account: account || '' }],
      last: { id, mailbox, account: account || '' },
    });
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
      this.clearMailSelection(false);

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
      .filter((el, i, arr) => {
        // Prefer is-checked set; if only one is-selected and no multi, still include
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
      }));
  },

  deleteSelectedMails() {
    const items = this.selectedMailItems();
    if (!items.length) return;
    const label =
      items.length === 1
        ? 'Supprimer ce message ?'
        : `Supprimer ${items.length} messages ?`;
    if (!window.confirm(label)) return;
    if (!window.htmx) return;
    window.htmx.ajax('POST', '/partials/message/delete-batch', {
      target: '#message-pane',
      swap: 'innerHTML',
      values: { items: JSON.stringify(items) },
    });
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
      const form = document.querySelector('#message-pane form[hx-post="/partials/message/delete"]');
      if (!form) {
        // Pas de message ouvert : supprimer la sélection simple
        if (this._mailSelected && this._mailSelected.size === 1) {
          this.deleteSelectedMails();
        }
        return;
      }
      const btn = form.querySelector('button[type="submit"]');
      if (btn) btn.click();
      else if (window.htmx) window.htmx.trigger(form, 'submit');
      else form.requestSubmit();
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

      if (ev.ctrlKey || ev.metaKey || ev.altKey) return;

      if (key === 'ArrowDown' || key === 'j') {
        if (!list.length) return;
        ev.preventDefault();
        const i = selectedIdx(list);
        const next = list[Math.min(list.length - 1, Math.max(0, i) + (i < 0 ? 0 : 1))];
        selectSingle(next);
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
        const payload = JSON.stringify({ id, account, mailbox });
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
           <button type="button" class="danger" data-ctx-action="delete" data-ctx-payload='${payload}'><i data-lucide="trash-2"></i> Supprimer</button>`
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
      return;
    }
    if (action === 'delete') {
      if (!window.confirm('Supprimer ce message ?')) return;
      if (window.htmx) {
        window.htmx.ajax('POST', '/partials/message/delete', {
          target: '#message-pane',
          swap: 'innerHTML',
          values: { mailbox: mb, id, account: acc },
        });
      }
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
  if (window.lucide) lucide.createIcons();
  const t = ev.detail && ev.detail.target;
  if (t && window.Alpine && typeof Alpine.initTree === 'function') {
    if (
      t.id === 'compose-layer' ||
      t.classList.contains('side-widget-body') ||
      (t.querySelector && t.querySelector('.side-widget-inner, .compose-backdrop'))
    ) {
      Alpine.initTree(t);
    }
  }
  if (t && (t.id === 'sidebar' || (t.querySelector && t.querySelector('#folder-nav')))) {
    window.HimaWeb.bindFolderClicks();
    window.HimaWeb.bindFolderTree(true);
  }
  // Fallback si les <script> du fragment ne s'exécutent pas
  if (t && t.id === 'message-pane' && window.HimaWeb) {
    const evEl = t.querySelector('[data-mail-event]');
    if (evEl && !window.HimaWeb._handlingMailEvent) {
      const kind = evEl.getAttribute('data-mail-event');
      const payload = {
        id: evEl.getAttribute('data-id') || '',
        mailbox: evEl.getAttribute('data-mailbox') || '',
        account: evEl.getAttribute('data-account') || '',
        to: evEl.getAttribute('data-to') || '',
      };
      if (kind === 'deleted') window.HimaWeb.onMessageDeleted(payload);
      else if (kind === 'moved') window.HimaWeb.onMessageMoved(payload);
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
  if (window.lucide) lucide.createIcons();
  window.HimaWeb.bindFolderClicks();
  window.HimaWeb.bindFolderTree(true);
  window.HimaWeb.bindColumnResize();
  window.HimaWeb.bindMailKeys();
  window.HimaWeb.bindContextMenus();
  window.HimaWeb.startUnreadPolling();
  try {
    const side = document.getElementById('side-widget');
    if (side && localStorage.getItem('himaweb-side-open') === '0') {
      side.setAttribute('data-open', '0');
    }
  } catch (_) {}
});
