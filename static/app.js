function composeForm(opts) {
  opts = opts || {};
  return {
    cardamum: !!opts.cardamum,
    showCc: !!(opts.cc || opts.bcc),
    to: opts.to || '',
    cc: opts.cc || '',
    bcc: opts.bcc || '',
    suggestions: { to: [], cc: [], bcc: [] },
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
      this[field] = item.email || item.label || '';
      this.suggestions[field] = [];
    },
  };
}

document.addEventListener('alpine:init', () => {
  if (window.Alpine) {
    Alpine.data('composeForm', composeForm);
  }
});


window.HimaWeb = {
  _lastUnreadTotal: null,
  _folderClicksBound: false,
  _folderTreeBound: false,
  _resizeBound: false,

  markEnvelopeRead(id, account) {
    const acc = account || '';
    document.querySelectorAll('.envelope.unread').forEach((el) => {
      if (el.dataset.id !== String(id)) return;
      if ((el.dataset.account || '') !== acc) return;
      el.classList.remove('unread');
      el.querySelectorAll('.from strong').forEach((s) => {
        const parent = s.parentNode;
        while (s.firstChild) parent.insertBefore(s.firstChild, s);
        s.remove();
      });
    });
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
      if (
        data.notifications &&
        this._lastUnreadTotal !== null &&
        total > this._lastUnreadTotal &&
        typeof Notification !== 'undefined'
      ) {
        if (Notification.permission === 'granted') {
          const delta = total - this._lastUnreadTotal;
          const first = (data.folders && data.folders[0]) || null;
          new Notification('HimaWeb', {
            body: first
              ? `${delta} nouveau(x) — ${first.label} (${first.unread})`
              : `${delta} nouveau(x) message(s) non lu(s)`,
            tag: 'himaweb-unread',
          });
        } else if (Notification.permission === 'default') {
          Notification.requestPermission();
        }
      }
      this._lastUnreadTotal = total;
    } catch (_) {}
  },
  startUnreadPolling() {
    this.pollUnread();
    setInterval(() => this.pollUnread(), 60000);
  },
};

function cssEsc(s) {
  if (window.CSS && CSS.escape) return CSS.escape(s);
  return String(s).replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

document.addEventListener('htmx:afterSwap', (ev) => {
  if (window.lucide) lucide.createIcons();
  const t = ev.detail && ev.detail.target;
  if (t && (t.id === 'sidebar' || (t.querySelector && t.querySelector('#folder-nav')))) {
    window.HimaWeb.bindFolderClicks();
    window.HimaWeb.bindFolderTree(true);
  }
});

document.addEventListener('DOMContentLoaded', () => {
  if (window.lucide) lucide.createIcons();
  window.HimaWeb.bindFolderClicks();
  window.HimaWeb.bindFolderTree(true);
  window.HimaWeb.bindColumnResize();
  window.HimaWeb.startUnreadPolling();
});
