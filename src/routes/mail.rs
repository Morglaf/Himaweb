use std::sync::Arc;

use askama::Template;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use serde::{Deserialize, Serialize};

use crate::cli::himalaya::Envelope;
use crate::prefs::Prefs;
use crate::attachments_class::is_accessory_attachment;
use crate::sanitize::{plain_to_html, remote_url_label, rewrite_cid_images, sanitize_html};
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/partials/sidebar", get(sidebar))
        .route("/partials/envelopes", get(envelopes))
        .route("/partials/message", get(message))
        .route("/partials/message/attachments", get(message_attachments))
        .route("/partials/message/flag", post(flag))
        .route("/partials/message/move", post(move_msg))
        .route("/partials/message/delete", post(delete_msg))
        .route("/partials/message/delete-batch", post(delete_batch))
        .route("/account/select", post(select_account))
        .route("/api/mail/unread", get(unread_counts))
        .route("/api/mail/move", post(move_api))
        .route("/api/mail/delete", post(delete_api))
        .route("/api/mail/undo", post(undo_api))
        .route("/mailboxes", get(sidebar))
}

#[derive(Deserialize)]
pub struct MailboxQuery {
    pub mailbox: Option<String>,
    pub account: Option<String>,
}

#[derive(Template)]
#[template(path = "sidebar.html")]
struct SidebarTemplate {
    pub accounts: Vec<AccountRow>,
    pub all_selected: bool,
    pub show_merged_inbox: bool,
    pub merged_inbox_active: bool,
    pub merged_inbox_unread: u64,
    pub pinned: Vec<MailboxRow>,
    pub mailboxes: Vec<MailboxRow>,
    pub current: String,
    pub offline: bool,
}

pub struct AccountRow {
    pub name: String,
    pub label: String,
    pub is_default: bool,
    pub selected: bool,
}

pub struct MailboxRow {
    pub name: String,
    pub name_enc: String,
    pub label: String,
    pub unread: u64,
    pub icon: String,
    pub active: bool,
    pub pinned: bool,
    pub depth: u8,
    pub pad: String,
    pub has_children: bool,
    pub parent: String,
    pub tree_id: String,
    pub account: String,
    pub account_enc: String,
    pub account_label: String,
    pub color: String,
    pub is_account_header: bool,
}

pub(crate) fn mailbox_icon(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    if n == "__ntfy__" || n.starts_with("__ntfy__:") {
        "bell"
    } else if n == "inbox" || n.ends_with("/inbox") {
        "inbox"
    } else if n.contains("sent") || n.contains("envoy") {
        "send"
    } else if n.contains("draft") {
        "file-pen"
    } else if n.contains("trash") || n.contains("corbeille") || n.contains("bin") {
        "trash-2"
    } else if n.contains("junk") || n.contains("spam") {
        "shield-alert"
    } else if n.contains("archive") {
        "archive"
    } else if n.contains("star") || n.contains("flag") {
        "star"
    } else {
        "folder"
    }
}

pub(crate) fn mailbox_label(name: &str) -> String {
    if name == "__ntfy__" || name.starts_with("__ntfy__:") {
        "Ntfy".into()
    } else {
        name.rsplit('/').next().unwrap_or(name).to_string()
    }
}

fn folder_rank(name: &str) -> u8 {
    let n = name.to_ascii_lowercase();
    if n == "inbox" {
        0
    } else if n.contains("star") {
        1
    } else if n.contains("draft") {
        2
    } else if n.contains("sent") {
        3
    } else if n.contains("archive") && !n.contains('/') {
        4
    } else if n.contains("junk") || n.contains("spam") {
        5
    } else if n.contains("trash") {
        6
    } else {
        10
    }
}

async fn sidebar(
    State(state): State<Arc<AppState>>,
    Query(q): Query<MailboxQuery>,
) -> impl IntoResponse {
    let current = q.mailbox.unwrap_or_else(|| "Inbox".into());
    let current_account = q.account.unwrap_or_default();
    let prefs_snap = state.prefs.lock().await.clone();
    let all_selected = prefs_snap.is_all_accounts();
    let selected_account = prefs_snap.account.clone().unwrap_or_default();
    let account_ref = prefs_snap.selected_account();

    // Permis relâché aussitôt : les phases suivantes acquièrent le leur, ce qui
    // leur permet de s'exécuter en parallèle.
    let account_infos = if state.himalaya_available {
        let _permit = state.cli_limit.acquire().await.ok();
        state.himalaya.list_accounts().await.unwrap_or_default()
    } else {
        vec![]
    };

    let known_names: Vec<String> = account_infos.iter().map(|a| a.name.clone()).collect();
    let ordered = prefs_snap.ordered_accounts(&known_names);
    let rail = prefs_snap.rail_order(&known_names);

    let accounts: Vec<AccountRow> = ordered
        .iter()
        .filter_map(|name| {
            account_infos.iter().find(|a| &a.name == name).map(|a| {
                let selected = !all_selected
                    && ((!selected_account.is_empty() && selected_account == a.name)
                        || (selected_account.is_empty() && a.is_default));
                AccountRow {
                    name: a.name.clone(),
                    label: prefs_snap.account_label(&a.name),
                    is_default: a.is_default,
                    selected,
                }
            })
        })
        .collect();

    let mut offline = false;
    let mut rows: Vec<MailboxRow> = Vec::new();
    // Dossiers surveillés dont Himalaya n'a pas fourni `unread` : à compter
    // ensuite, en parallèle, plutôt que de relancer une recherche à chaque fois.
    let mut need_count: Vec<(usize, String, Option<String>)> = Vec::new();

    if all_selected {
        // Un `list_mailboxes` par compte, tous en parallèle. L'ordre d'arrivée
        // des tâches est arbitraire : on reconstruit ensuite selon rail_order.
        let mut boxes_tasks = tokio::task::JoinSet::new();
        if state.himalaya_available {
            for acc_name in rail.iter().filter(|a| !Prefs::is_ntfy_key(a)).cloned() {
                let st = Arc::clone(&state);
                boxes_tasks.spawn(async move {
                    let _permit = st.cli_limit.acquire().await.ok();
                    let res = st.himalaya.list_mailboxes(Some(&acc_name)).await;
                    (acc_name, res)
                });
            }
        }

        let mut boxes_by_account: std::collections::HashMap<String, Vec<_>> =
            std::collections::HashMap::new();
        let mut any_ok = false;
        while let Some(joined) = boxes_tasks.join_next().await {
            let Ok((acc_name, res)) = joined else {
                continue;
            };
            match res {
                Ok(list) => {
                    any_ok = true;
                    boxes_by_account.insert(acc_name, list);
                }
                Err(_) => offline = true,
            }
        }

        // Un arbre par compte (+ ntfy) selon rail_order
        for acc_name in &rail {
            if Prefs::is_ntfy_key(acc_name) {
                if let Some(row) = ntfy_sidebar_row(&prefs_snap, acc_name, &current) {
                    rows.push(row);
                }
                continue;
            }
            let color = prefs_snap.account_color(acc_name);
            let boxes = boxes_by_account.remove(acc_name).unwrap_or_default();
            if boxes.is_empty() {
                continue;
            }

            let header_id = format!("@acc@{acc_name}");
            rows.push(MailboxRow {
                name: String::new(),
                name_enc: String::new(),
                label: prefs_snap.account_label(acc_name),
                unread: 0,
                icon: prefs_snap.account_icon(acc_name),
                active: false,
                pinned: false,

                depth: 0,
                pad: "0.75rem".into(),
                has_children: true,
                parent: String::new(),
                tree_id: header_id.clone(),
                account: acc_name.clone(),
                account_enc: urlencoding::encode(acc_name).into_owned(),
                account_label: prefs_snap.account_label(acc_name),
                color: color.clone(),
                is_account_header: true,
            });

            for m in boxes {
                let key = Prefs::folder_key(Some(acc_name), &m.name);
                if prefs_snap.is_hidden(&key) {
                    continue;
                }
                let depth = (1 + m.name.matches('/').count().min(7)) as u8;
                let pad = format!("{:.2}rem", 0.75 + f32::from(depth) * 0.85);
                let parent = if let Some((p, _)) = m.name.rsplit_once('/') {
                    format!("{acc_name}::{p}")
                } else {
                    header_id.clone()
                };
                let tree_id = format!("{acc_name}::{}", m.name);
                let active = m.name.eq_ignore_ascii_case(&current)
                    && (current_account.is_empty() || current_account == *acc_name);
                let unread = m.unread.unwrap_or(0);
                let idx = rows.len();
                if m.unread.is_none() {
                    let watched = prefs_snap.is_watched(&key, &m.name);
                    if watched {
                        need_count.push((idx, m.name.clone(), Some(acc_name.clone())));
                    }
                }
                rows.push(MailboxRow {
                    icon: mailbox_icon(&m.name).into(),
                    label: mailbox_label(&m.name),
                    name_enc: urlencoding::encode(&m.name).into_owned(),
                    pinned: prefs_snap.is_pinned(&key),

                    name: m.name.clone(),
                    unread,
                    active,
                    depth,
                    pad,
                    has_children: false,
                    parent,
                    tree_id,
                    account: acc_name.clone(),
                    account_enc: urlencoding::encode(acc_name).into_owned(),
                    account_label: prefs_snap.account_label(acc_name),
                    color: color.clone(),
                    is_account_header: false,
                });
            }
        }
        if !any_ok && state.himalaya_available {
            offline = true;
        }
    } else {
        let (boxes, off) = if state.himalaya_available {
            let _permit = state.cli_limit.acquire().await.ok();
            match state.himalaya.list_mailboxes(account_ref).await {
                Ok(list) => {
                    let cache = state.cache.lock().await;
                    let _ = cache.save_mailboxes(&list);
                    (list, false)
                }
                Err(e) => {
                    tracing::warn!("mailbox list online échoué: {e}");
                    let cache = state.cache.lock().await;
                    (cache.load_mailboxes().unwrap_or_default(), true)
                }
            }
        } else {
            let cache = state.cache.lock().await;
            (cache.load_mailboxes().unwrap_or_default(), true)
        };
        offline = off;
        let color = account_ref
            .map(|a| prefs_snap.account_color(a))
            .unwrap_or_else(|| "#64748b".into());
        let acc_label = account_ref.unwrap_or("").to_string();

        for m in boxes {
            let key = Prefs::folder_key(account_ref, &m.name);
            if prefs_snap.is_hidden(&key) {
                continue;
            }
            let active = m.name.eq_ignore_ascii_case(&current);
            let depth = m.name.matches('/').count().min(8) as u8;
            let pad = format!("{:.2}rem", 0.75 + f32::from(depth) * 0.85);
            let parent = if let Some((p, _)) = m.name.rsplit_once('/') {
                p.to_string()
            } else {
                String::new()
            };
            let unread = m.unread.unwrap_or(0);
            let idx = rows.len();
            if m.unread.is_none() {
                if prefs_snap.is_watched(&key, &m.name) {
                    need_count.push((
                        idx,
                        m.name.clone(),
                        account_ref.map(str::to_string),
                    ));
                }
            }
            rows.push(MailboxRow {
                icon: mailbox_icon(&m.name).into(),
                label: mailbox_label(&m.name),
                name_enc: urlencoding::encode(&m.name).into_owned(),
                pinned: prefs_snap.is_pinned(&key),

                tree_id: m.name.clone(),
                name: m.name,
                unread,
                active,
                depth,
                pad,
                has_children: false,
                parent,
                account: acc_label.clone(),
                account_enc: urlencoding::encode(&acc_label).into_owned(),
                account_label: if acc_label.is_empty() {
                    String::new()
                } else {
                    prefs_snap.account_label(&acc_label)
                },
                color: color.clone(),
                is_account_header: false,
            });
        }
        // En mode compte unique : intercaler les ntfy selon rail_order (après les dossiers mail)
        for key in prefs_snap.ntfy_order_keys() {
            if let Some(row) = ntfy_sidebar_row(&prefs_snap, &key, &current) {
                rows.push(row);
            }
        }
    }

    // Himalaya renvoie souvent `unread: null` : il faut alors une recherche
    // IMAP par dossier surveillé. C'était la boucle séquentielle la plus
    // coûteuse de la sidebar ; les appels partent maintenant ensemble, et
    // seulement quand le compteur n'était pas déjà fourni.
    if state.himalaya_available && !need_count.is_empty() {
        let mut count_tasks = tokio::task::JoinSet::new();
        for (idx, mailbox, acc) in need_count {
            let st = Arc::clone(&state);
            count_tasks.spawn(async move {
                let _permit = st.cli_limit.acquire().await.ok();
                let n = st.himalaya.count_unseen(&mailbox, acc.as_deref()).await.ok();
                (idx, n)
            });
        }
        while let Some(joined) = count_tasks.join_next().await {
            if let Ok((idx, Some(n))) = joined {
                if let Some(row) = rows.get_mut(idx) {
                    row.unread = n;
                }
            }
        }
    }

    // Compteurs non-lus NTFY (si surveillés)
    for r in &mut rows {
        if Prefs::is_ntfy_key(&r.name) {
            let fk = Prefs::ntfy_folder_key(&r.name);
            if prefs_snap.is_watched(&fk, &r.name) {
                r.unread = ntfy_unread_count(&prefs_snap, &r.name).await;
            }
        }
    }

    // Marquer les parents qui ont des enfants (via parent == tree_id)
    let parents_with_kids: std::collections::HashSet<String> =
        rows.iter().map(|r| r.parent.clone()).filter(|p| !p.is_empty()).collect();
    for r in &mut rows {
        r.has_children = parents_with_kids.contains(&r.tree_id);
    }

    if !all_selected {
        rows.sort_by(|a, b| {
            // ntfy après les dossiers mail, ordre relatif des clés ntfy
            let a_ntfy = Prefs::is_ntfy_key(&a.name);
            let b_ntfy = Prefs::is_ntfy_key(&b.name);
            match (a_ntfy, b_ntfy) {
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (true, true) => a.name.cmp(&b.name),
                (false, false) => {
                    let ar = if a.name.contains('/') {
                        folder_rank(a.name.split('/').next().unwrap_or(&a.name))
                    } else {
                        folder_rank(&a.name)
                    };
                    let br = if b.name.contains('/') {
                        folder_rank(b.name.split('/').next().unwrap_or(&b.name))
                    } else {
                        folder_rank(&b.name)
                    };
                    ar.cmp(&br)
                        .then(a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
                }
            }
        });
    } else {
        let order_idx = |name: &str| {
            rail.iter()
                .position(|n| n == name)
                .unwrap_or(usize::MAX)
        };
        rows.sort_by(|a, b| {
            order_idx(&a.account)
                .cmp(&order_idx(&b.account))
                .then_with(|| b.is_account_header.cmp(&a.is_account_header))
                .then_with(|| {
                    let an = if a.is_account_header {
                        ""
                    } else {
                        a.name.as_str()
                    };
                    let bn = if b.is_account_header {
                        ""
                    } else {
                        b.name.as_str()
                    };
                    let ar = folder_rank(an.split('/').next().unwrap_or(an));
                    let br = folder_rank(bn.split('/').next().unwrap_or(bn));
                    ar.cmp(&br)
                        .then(an.to_ascii_lowercase().cmp(&bn.to_ascii_lowercase()))
                })
        });
    }

    let show_merged_inbox = all_selected && prefs_snap.merged_inbox;
    let merged_inbox_active = show_merged_inbox
        && current.eq_ignore_ascii_case("inbox")
        && current_account.is_empty();
    let merged_inbox_unread: u64 = if show_merged_inbox {
        rows.iter()
            .filter(|r| {
                !r.is_account_header
                    && !Prefs::is_ntfy_key(&r.name)
                    && (r.name.eq_ignore_ascii_case("inbox")
                        || r.name.to_ascii_lowercase().ends_with("/inbox"))
            })
            .map(|r| r.unread)
            .sum()
    } else {
        0
    };

    let mut pinned = Vec::new();
    let mut mailboxes = Vec::new();
    for r in rows {
        if r.pinned && !r.is_account_header {
            pinned.push(r);
        } else {
            mailboxes.push(r);
        }
    }

    render(SidebarTemplate {
        accounts,
        all_selected,
        show_merged_inbox,
        merged_inbox_active,
        merged_inbox_unread,
        pinned,
        mailboxes,
        current,
        offline,
    })
}

fn ntfy_sidebar_row(prefs: &Prefs, key: &str, current: &str) -> Option<MailboxRow> {
    let folder_key = Prefs::ntfy_folder_key(key);
    if prefs.is_hidden(&folder_key) {
        return None;
    }
    Some(MailboxRow {
        name: key.to_string(),
        name_enc: urlencoding::encode(key).into_owned(),
        label: prefs.account_label(key),
        unread: 0, // renseigné après poll si surveillé
        icon: prefs.account_icon(key),
        active: current == key,
        pinned: prefs.is_pinned(&folder_key),
        depth: 0,
        pad: "0.75rem".into(),
        has_children: false,
        parent: String::new(),
        tree_id: key.to_string(),
        account: key.to_string(),
        account_enc: urlencoding::encode(key).into_owned(),
        account_label: prefs.account_label(key),
        color: prefs.account_color(key),
        is_account_header: false,
    })
}

async fn ntfy_unread_count(prefs: &Prefs, mailbox_key: &str) -> u64 {
    let mut n = 0u64;
    for src in prefs.ntfy_sources_for_key(mailbox_key) {
        match crate::plugins::ntfy_poll(&src.server, &src.topic).await {
            Ok(msgs) => {
                for m in msgs {
                    let composite = format!("{}::{}", src.id, m.id);
                    if prefs.is_ntfy_deleted(&composite) {
                        continue;
                    }
                    if !prefs.is_ntfy_read(&composite) {
                        n += 1;
                    }
                }
            }
            Err(_) => {}
        }
    }
    n
}

#[derive(Deserialize)]
pub struct PageQuery {
    pub mailbox: Option<String>,
    pub page: Option<u32>,
    pub account: Option<String>,
    pub q: Option<String>,
    pub sort: Option<String>,
    /// Ne rendre que les lignes + la sentinelle (chargement progressif).
    pub append: Option<u8>,
    /// Ignorer le cache et interroger Himalaya.
    pub fresh: Option<u8>,
    /// Nombre de pages déjà chargées automatiquement au scroll.
    pub auto_pages: Option<u32>,
}

/// Au-delà de ce nombre de pages enchaînées au scroll, la sentinelle redevient
/// purement manuelle : on évite de tirer des milliers de messages par mégarde.
const AUTO_PAGES_MAX: u32 = 5;
const ENVELOPE_PAGE_SIZE: u32 = 50;

#[derive(Template)]
#[template(path = "envelopes.html")]
struct EnvelopesTemplate {
    pub mailbox: String,
    pub mailbox_enc: String,
    pub next_page: Option<u32>,
    pub envelopes: Vec<EnvelopeRow>,
    pub offline: bool,
    pub error: Option<String>,
    pub all_mode: bool,
    pub query: String,
    /// Réponse d'un « voir plus » : pas de bandeaux ni d'état vide.
    pub rows_only: bool,
    /// Liste servie depuis le cache, rafraîchissement déclenché côté client.
    pub stale: bool,
    /// Paramètres communs aux URLs de pagination (hors page/append/auto_pages).
    pub base_qs: String,
    pub next_auto_pages: u32,
    pub auto_load: bool,
}

#[derive(Clone)]
pub struct EnvelopeRow {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub from_initial: String,
    pub date: String,
    pub date_short: String,
    pub unread: bool,
    pub has_attachment: bool,
    pub account: String,
    pub account_enc: String,
    pub account_label: String,
    pub account_icon: String,
    pub color: String,
    /// Nombre de messages dans la conversation (≥ 1)
    pub thread_count: u32,
    /// Aperçu participants (conversations)
    pub participants: String,
    /// IDs du fil (ordre chrono croissant), séparés par des virgules
    pub thread_ids: String,
    pub thread_ids_enc: String,
    /// Message-ID RFC (pour undo)
    pub message_id: String,
    pub id_enc: String,
}

fn envelope_rows(list: &[Envelope], account: &str, prefs: &Prefs) -> Vec<EnvelopeRow> {
    let color = prefs.account_color(account);
    let label = if account.is_empty() {
        String::new()
    } else {
        prefs.account_label(account)
    };
    let icon = if account.is_empty() {
        "circle-user".into()
    } else {
        prefs.account_icon(account)
    };
    let rows = envelope_rows_styled(list, account, &color, &label, &icon);
    if prefs.conversations {
        collapse_conversations(list, rows)
    } else {
        rows
    }
}

fn envelope_rows_styled(
    list: &[Envelope],
    account: &str,
    color: &str,
    account_label: &str,
    account_icon: &str,
) -> Vec<EnvelopeRow> {
    list.iter()
        .map(|e| {
            let unread = !e.flags.iter().any(|f| {
                let x = f.to_ascii_lowercase();
                x == "seen" || x == "\\seen"
            });
            EnvelopeRow {
                id: e.id.clone(),
                subject: e.subject.clone(),
                from_initial: e
                    .from
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string(),
                from: e.from.clone(),
                to: e.to.clone(),
                date: e.date.clone(),
                date_short: short_date(&e.date),
                unread,
                has_attachment: e.has_attachment,
                account: account.to_string(),
                account_enc: urlencoding::encode(account).into_owned(),
                account_label: account_label.to_string(),
                account_icon: account_icon.to_string(),
                color: color.to_string(),
                thread_count: 1,
                participants: String::new(),
                thread_ids: e.id.clone(),
                thread_ids_enc: urlencoding::encode(&e.id).into_owned(),
                message_id: e.message_id.clone(),
                id_enc: urlencoding::encode(&e.id).into_owned(),
            }
        })
        .collect()
}

fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim().to_lowercase();
    loop {
        let before = s.clone();
        for p in [
            "re:", "fwd:", "fw:", "aw:", "sv:", "tr:", "ré:", "rép:", "réponse:", "reponse:",
        ] {
            if let Some(rest) = s.strip_prefix(p) {
                s = rest.trim_start().to_string();
            }
        }
        // "[tag] " prefixes sometimes
        if s.starts_with('[') {
            if let Some(end) = s.find(']') {
                s = s[end + 1..].trim_start().to_string();
                continue;
            }
        }
        if s == before {
            break;
        }
    }
    s
}

/// Regroupe par Message-ID / In-Reply-To / References, puis par sujet normalisé.
fn collapse_conversations(list: &[Envelope], rows: Vec<EnvelopeRow>) -> Vec<EnvelopeRow> {
    if list.is_empty() || rows.len() != list.len() {
        return rows;
    }
    let n = list.len();
    let mut parent: Vec<usize> = (0..n).collect();
    let find = |parent: &mut [usize], mut i: usize| -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    };
    let union = |parent: &mut [usize], a: usize, b: usize| {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[rb] = ra;
        }
    };

    use std::collections::HashMap;
    let mut by_msgid: HashMap<String, usize> = HashMap::new();
    for (i, e) in list.iter().enumerate() {
        if !e.message_id.is_empty() {
            by_msgid.insert(e.message_id.clone(), i);
        }
    }
    for (i, e) in list.iter().enumerate() {
        for id in e.in_reply_to.iter().chain(e.references.iter()) {
            if let Some(&j) = by_msgid.get(id) {
                union(&mut parent, i, j);
            }
        }
    }
    // Sujet normalisé (même compte / boîte déjà homogène dans `list`)
    let mut by_subj: HashMap<String, usize> = HashMap::new();
    for (i, e) in list.iter().enumerate() {
        let key = normalize_subject(&e.subject);
        if key.is_empty() {
            continue;
        }
        if let Some(&j) = by_subj.get(&key) {
            union(&mut parent, i, j);
        } else {
            by_subj.insert(key, i);
        }
    }

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }

    let mut out: Vec<EnvelopeRow> = Vec::with_capacity(groups.len());
    for mut idxs in groups.into_values() {
        idxs.sort_by(|&a, &b| list[b].date.cmp(&list[a].date));
        let head = idxs[0];
        let mut row = rows[head].clone();
        row.thread_count = idxs.len() as u32;
        // Prefer first unread as open target if any
        if let Some(&u) = idxs.iter().find(|&&i| {
            !list[i].flags.iter().any(|f| {
                let x = f.to_ascii_lowercase();
                x == "seen" || x == "\\seen"
            })
        }) {
            row.id = list[u].id.clone();
            row.id_enc = urlencoding::encode(&list[u].id).into_owned();
            row.unread = true;
            row.from = list[u].from.clone();
            row.message_id = list[u].message_id.clone();
            row.from_initial = list[u]
                .from
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string();
            row.date = list[u].date.clone();
            row.date_short = short_date(&list[u].date);
            row.has_attachment = idxs.iter().any(|&i| list[i].has_attachment);
        } else {
            row.has_attachment = idxs.iter().any(|&i| list[i].has_attachment);
        }
        // Clean subject display (without endless Re:)
        let clean = normalize_subject(&list[head].subject);
        if !clean.is_empty() {
            // restore light capitalization from original if possible
            row.subject = list[head].subject.clone();
            // strip leading Re:/Fwd: for display
            let mut display = list[head].subject.trim().to_string();
            loop {
                let before = display.clone();
                for p in ["Re:", "RE:", "Fwd:", "FWD:", "Fw:", "Aw:", "SV:"] {
                    if let Some(rest) = display.strip_prefix(p) {
                        display = rest.trim_start().to_string();
                    }
                }
                if display == before {
                    break;
                }
            }
            if !display.is_empty() {
                row.subject = display;
            }
        }
        let mut parts: Vec<String> = Vec::new();
        for &i in &idxs {
            let name = list[i]
                .from
                .split('<')
                .next()
                .unwrap_or(&list[i].from)
                .trim()
                .trim_matches('"')
                .to_string();
            if !name.is_empty() && !parts.iter().any(|p| p.eq_ignore_ascii_case(&name)) {
                parts.push(name);
            }
            if parts.len() >= 3 {
                break;
            }
        }
        row.participants = parts.join(", ");
        let mut chrono = idxs.clone();
        chrono.sort_by(|&a, &b| list[a].date.cmp(&list[b].date));
        row.thread_ids = chrono
            .iter()
            .map(|&i| list[i].id.as_str())
            .collect::<Vec<_>>()
            .join(",");
        // Encoder chaque UID séparément ; garder les virgules littérales
        // (sinon %2C n'est pas re-découpé et on n'affiche qu'un message).
        row.thread_ids_enc = chrono
            .iter()
            .map(|&i| urlencoding::encode(&list[i].id).into_owned())
            .collect::<Vec<_>>()
            .join(",");
        out.push(row);
    }
    out.sort_by(|a, b| b.date.cmp(&a.date));
    out
}

fn short_date(date: &str) -> String {
    let date = date.trim();
    if date.is_empty() || date.eq_ignore_ascii_case("null") {
        return "—".into();
    }
    // ISO-8601 : 2026-09-28T17:59:00+02:00 ou avec espace
    if date.len() >= 16 && date.as_bytes().get(4) == Some(&b'-') {
        let day = &date[0..10];
        let time = date.get(11..16).unwrap_or("").trim_end_matches('Z');
        if !time.is_empty() && time.as_bytes().get(2) == Some(&b':') {
            return format!("{day} {time}");
        }
        return day.to_string();
    }
    date.to_string()
}

/// Tri des listes / recherches Himalaya (`order by …`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortField {
    Date,
    From,
    To,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SortSpec {
    pub field: SortField,
    pub dir: SortDir,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            field: SortField::Date,
            dir: SortDir::Desc,
        }
    }
}

impl SortSpec {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim).unwrap_or("date_desc") {
            "date_asc" => Self {
                field: SortField::Date,
                dir: SortDir::Asc,
            },
            "from_asc" => Self {
                field: SortField::From,
                dir: SortDir::Asc,
            },
            "from_desc" => Self {
                field: SortField::From,
                dir: SortDir::Desc,
            },
            "to_asc" => Self {
                field: SortField::To,
                dir: SortDir::Asc,
            },
            "to_desc" => Self {
                field: SortField::To,
                dir: SortDir::Desc,
            },
            _ => Self::default(),
        }
    }

    pub fn as_str(self) -> &'static str {
        match (self.field, self.dir) {
            (SortField::Date, SortDir::Desc) => "date_desc",
            (SortField::Date, SortDir::Asc) => "date_asc",
            (SortField::From, SortDir::Asc) => "from_asc",
            (SortField::From, SortDir::Desc) => "from_desc",
            (SortField::To, SortDir::Asc) => "to_asc",
            (SortField::To, SortDir::Desc) => "to_desc",
        }
    }

    pub fn is_default(self) -> bool {
        self == Self::default()
    }

    pub fn order_tokens(self) -> Vec<String> {
        let field = match self.field {
            SortField::Date => "date",
            SortField::From => "from",
            SortField::To => "to",
        };
        let dir = match self.dir {
            SortDir::Asc => "asc",
            SortDir::Desc => "desc",
        };
        vec!["order".into(), "by".into(), field.into(), dir.into()]
    }
}

pub(crate) fn sort_envelopes(list: &mut [Envelope], sort: SortSpec) {
    list.sort_by(|a, b| {
        let ord = match sort.field {
            SortField::Date => a.date.cmp(&b.date),
            SortField::From => a
                .from
                .to_ascii_lowercase()
                .cmp(&b.from.to_ascii_lowercase()),
            SortField::To => a.to.to_ascii_lowercase().cmp(&b.to.to_ascii_lowercase()),
        };
        match sort.dir {
            SortDir::Asc => ord,
            SortDir::Desc => ord.reverse(),
        }
    });
}

pub(crate) fn sort_envelope_rows(rows: &mut [EnvelopeRow], sort: SortSpec) {
    rows.sort_by(|a, b| {
        let ord = match sort.field {
            SortField::Date => a.date.cmp(&b.date),
            SortField::From => a
                .from
                .to_ascii_lowercase()
                .cmp(&b.from.to_ascii_lowercase()),
            SortField::To => a
                .to
                .to_ascii_lowercase()
                .cmp(&b.to.to_ascii_lowercase()),
        };
        match sort.dir {
            SortDir::Asc => ord,
            SortDir::Desc => ord.reverse(),
        }
    });
}

/// Construit les tokens DSL Himalaya pour une recherche libre.
/// Par défaut : `from` + `subject` (rapide). `include_body` ajoute `body` (souvent très lent en IMAP).
pub(crate) fn search_query_tokens(raw: &str, include_body: bool) -> Vec<String> {
    let q = raw.trim();
    if q.is_empty() {
        return vec![];
    }
    // Échapper guillemets ; Himalaya accepte des patterns simples
    let safe: String = q
        .chars()
        .map(|c| if c == '"' || c == '\\' { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if safe.is_empty() {
        return vec![];
    }
    let mut tokens = vec![
        "from".into(),
        safe.clone(),
        "or".into(),
        "subject".into(),
        safe.clone(),
    ];
    if include_body {
        tokens.push("or".into());
        tokens.push("body".into());
        tokens.push(safe);
    }
    tokens
}

async fn fetch_envelopes_for_account(
    state: &AppState,
    mailbox: &str,
    page: u32,
    page_size: u32,
    account: Option<&str>,
    search_tokens: &[String],
    sort: SortSpec,
) -> Result<Vec<crate::cli::himalaya::Envelope>, crate::cli::runner::CliError> {
    let use_search = !search_tokens.is_empty() || !sort.is_default();
    let mut list = if !use_search {
        state
            .himalaya
            .list_envelopes(mailbox, page, page_size, account)
            .await?
    } else {
        let mut tokens = search_tokens.to_vec();
        tokens.extend(sort.order_tokens());
        let refs: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();
        state
            .himalaya
            .search_envelopes(mailbox, &refs, page, page_size, account)
            .await?
    };
    sort_envelopes(&mut list, sort);
    Ok(list)
}

async fn envelopes(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PageQuery>,
) -> impl IntoResponse {
    let name = q.mailbox.unwrap_or_else(|| "Inbox".into());
    let page = q.page.unwrap_or(1).max(1);
    let page_size = ENVELOPE_PAGE_SIZE;
    let query = q.q.unwrap_or_default();
    let sort = SortSpec::parse(q.sort.as_deref());
    let search_tokens = search_query_tokens(&query, false);
    let prefs_snap = state.prefs.lock().await.clone();
    let rows_only = q.append.unwrap_or(0) == 1;
    let want_fresh = q.fresh.unwrap_or(0) == 1;
    let auto_pages = q.auto_pages.unwrap_or(0);

    if Prefs::is_ntfy_key(&name) {
        return ntfy_envelopes(
            &prefs_snap,
            &name,
            &query,
            page,
            page_size,
            rows_only,
            auto_pages,
        )
        .await;
    }

    let all_mode = prefs_snap.is_all_accounts();
    let account = prefs_snap.selected_account().map(str::to_string);
    let filter_account = q
        .account
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let mailbox_enc = urlencoding::encode(&name).into_owned();
    let query_enc = urlencoding::encode(&query).into_owned();
    let account_filter = filter_account.clone().unwrap_or_default();
    let account_filter_enc = urlencoding::encode(&account_filter).into_owned();
    let sort_s = sort.as_str().to_string();

    let mut base_qs = format!("mailbox={mailbox_enc}&sort={sort_s}");
    if !query_enc.is_empty() {
        base_qs.push_str(&format!("&q={query_enc}"));
    }
    if !account_filter_enc.is_empty() {
        base_qs.push_str(&format!("&account={account_filter_enc}"));
    }

    // Comptes concernés, dans l'ordre d'affichage.
    let targets: Vec<Option<String>> = if state.himalaya_available && all_mode {
        let infos = {
            let _permit = state.cli_limit.acquire().await.ok();
            state.himalaya.list_accounts().await.unwrap_or_default()
        };
        let known: Vec<String> = infos.iter().map(|a| a.name.clone()).collect();
        let ordered = prefs_snap.ordered_accounts(&known);
        let picked = match &filter_account {
            Some(fa) if ordered.iter().any(|a| a == fa) => vec![fa.clone()],
            _ => ordered,
        };
        picked.into_iter().map(Some).collect()
    } else {
        vec![account.clone()]
    };

    // Le cache ne couvre que la première page d'un dossier, sans recherche ni
    // tri personnalisé : en dehors de ce cas, il ne peut rien servir.
    let cacheable = page == 1 && search_tokens.is_empty() && sort.is_default();

    let (rows, offline, error, has_next, stale) = if !state.himalaya_available {
        let (rows, _, has_next) =
            load_envelopes_from_cache(&state, &targets, &name, &prefs_snap, sort, page_size).await;
        (rows, true, None, has_next, false)
    } else if cacheable && !want_fresh && !rows_only {
        // Chemin instantané : si le cache connaît ce dossier, on le rend tout de
        // suite et le client redemande la version fraîche (voir `stale`).
        let (cached_rows, found, has_next) =
            load_envelopes_from_cache(&state, &targets, &name, &prefs_snap, sort, page_size).await;
        if found {
            (cached_rows, false, None, has_next, true)
        } else {
            let fetched =
                fetch_envelopes_online(&state, &targets, &name, page, page_size, &search_tokens, sort, cacheable, &prefs_snap)
                    .await;
            (fetched.0, fetched.1, fetched.2, fetched.3, false)
        }
    } else {
        let fetched = fetch_envelopes_online(
            &state,
            &targets,
            &name,
            page,
            page_size,
            &search_tokens,
            sort,
            cacheable,
            &prefs_snap,
        )
        .await;
        (fetched.0, fetched.1, fetched.2, fetched.3, false)
    };

    render(EnvelopesTemplate {
        mailbox_enc,
        mailbox: name,
        next_page: if has_next { Some(page + 1) } else { None },
        envelopes: rows,
        offline,
        error,
        all_mode,
        query,
        rows_only,
        stale,
        base_qs,
        next_auto_pages: auto_pages.saturating_add(1),
        auto_load: auto_pages < AUTO_PAGES_MAX,
    })
}

/// Enveloppes du cache SQLite pour les comptes demandés, fusionnées et triées.
///
/// Le premier booléen indique si au moins un compte avait une entrée en cache
/// (un dossier vraiment vide et un dossier jamais synchronisé se ressemblent).
/// Le second est `has_next` **par compte** : une page pleine chez n'importe
/// lequel des comptes suffit.
async fn load_envelopes_from_cache(
    state: &AppState,
    targets: &[Option<String>],
    mailbox: &str,
    prefs_snap: &Prefs,
    sort: SortSpec,
    page_size: u32,
) -> (Vec<EnvelopeRow>, bool, bool) {
    let mut merged: Vec<EnvelopeRow> = Vec::new();
    let mut found = false;
    let mut has_next = false;
    {
        let cache = state.cache.lock().await;
        for acc in targets {
            let acc_name = acc.as_deref().unwrap_or("");
            let mut list = cache.load_envelopes(acc_name, mailbox).unwrap_or_default();
            if list.is_empty() {
                continue;
            }
            found = true;
            if list.len() as u32 >= page_size {
                has_next = true;
            }
            sort_envelopes(&mut list, sort);
            merged.extend(envelope_rows(&list, acc_name, prefs_snap));
        }
    }
    sort_envelope_rows(&mut merged, sort);
    (merged, found, has_next)
}

/// Enveloppes via Himalaya, un appel par compte, tous lancés ensemble.
///
/// Retourne `(lignes, hors-ligne, erreur, page suivante probable)`. `has_next`
/// est évalué **par compte** : en mode fusionné, comparer le total agrégé à
/// `page_size` donnerait toujours vrai dès qu'il y a plusieurs comptes.
#[allow(clippy::too_many_arguments)]
async fn fetch_envelopes_online(
    state: &Arc<AppState>,
    targets: &[Option<String>],
    mailbox: &str,
    page: u32,
    page_size: u32,
    search_tokens: &[String],
    sort: SortSpec,
    cacheable: bool,
    prefs_snap: &Prefs,
) -> (Vec<EnvelopeRow>, bool, Option<String>, bool) {
    let mut tasks = tokio::task::JoinSet::new();
    for acc in targets.iter().cloned() {
        let st = Arc::clone(state);
        let mailbox = mailbox.to_string();
        let tokens = search_tokens.to_vec();
        tasks.spawn(async move {
            let _permit = st.cli_limit.acquire().await.ok();
            let res = fetch_envelopes_for_account(
                &st,
                &mailbox,
                page,
                page_size,
                acc.as_deref(),
                &tokens,
                sort,
            )
            .await;
            (acc, res)
        });
    }

    let mut lists: std::collections::HashMap<String, Vec<Envelope>> =
        std::collections::HashMap::new();
    // BTreeMap : message d'erreur stable malgré l'ordre d'arrivée des tâches.
    let mut errs: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut offline_any = false;
    let mut has_next = false;

    while let Some(joined) = tasks.join_next().await {
        let Ok((acc, res)) = joined else {
            continue;
        };
        let acc_name = acc.as_deref().unwrap_or("").to_string();
        match res {
            Ok(list) => {
                if list.len() as u32 >= page_size {
                    has_next = true;
                }
                if cacheable {
                    let cache = state.cache.lock().await;
                    let _ = cache.save_envelopes(&acc_name, mailbox, &list);
                }
                lists.insert(acc_name, list);
            }
            Err(e) => {
                tracing::warn!("envelope list/search échoué ({acc_name}): {e}");
                offline_any = true;
                errs.insert(acc_name, e.to_string());
            }
        }
    }

    // Reconstruction dans l'ordre des comptes, puis tri global.
    let mut merged: Vec<EnvelopeRow> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for acc in targets {
        let acc_name = acc.as_deref().unwrap_or("");
        let Some(list) = lists.get(acc_name) else {
            continue;
        };
        for row in envelope_rows(list, acc_name, prefs_snap) {
            if seen.insert(format!("{}::{}", row.account, row.id)) {
                merged.push(row);
            }
        }
    }
    sort_envelope_rows(&mut merged, sort);

    // Repli hors-ligne : si tout a échoué, servir ce que le cache contient.
    if merged.is_empty() && offline_any && search_tokens.is_empty() {
        let (cached, _, has_next) =
            load_envelopes_from_cache(state, targets, mailbox, prefs_snap, sort, page_size).await;
        if !cached.is_empty() {
            let error = Some(
                errs.into_iter()
                    .map(|(a, e)| if a.is_empty() { e } else { format!("{a}: {e}") })
                    .collect::<Vec<_>>()
                    .join(" · "),
            );
            return (cached, true, error, has_next);
        }
    }

    let error = if errs.is_empty() {
        None
    } else {
        Some(
            errs.into_iter()
                .map(|(a, e)| if a.is_empty() { e } else { format!("{a}: {e}") })
                .collect::<Vec<_>>()
                .join(" · "),
        )
    };
    (merged, offline_any, error, has_next)
}

#[derive(Deserialize)]
pub struct MessageQuery {
    pub mailbox: String,
    pub id: String,
    pub account: Option<String>,
    /// IDs du fil (séparés par virgules), ordre chrono
    pub thread: Option<String>,
    /// 1 = alimenter le cache sans rendre le HTML (survol).
    pub prefetch: Option<u8>,
}

fn parse_thread_ids(raw: &str) -> Vec<String> {
    // Tolère un éventuel %2C résiduel (double encodage) en plus des virgules.
    let normalized = raw.replace("%2C", ",").replace("%2c", ",");
    normalized
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[derive(Template)]
#[template(path = "message.html")]
struct MessageTemplate {
    pub mailbox: String,
    pub mailbox_enc: String,
    pub id: String,
    pub subject: String,
    pub account: String,
    pub account_enc: String,
    pub mailboxes: Vec<MoveOpt>,
    pub offline: bool,
    pub error: Option<String>,
    pub marked_read: bool,
    pub thread_count: u32,
    pub thread: Vec<ThreadPart>,
}

pub struct ThreadPart {
    pub id: String,
    pub subject: String,
    pub from: String,
    pub from_initial: String,
    pub to: String,
    pub cc: String,
    pub date: String,
    pub unread: bool,
    pub body: String,
    pub attachments: Vec<AttRow>,
    pub accessory_attachments: Vec<AttRow>,
    pub remote_resources: Vec<RemoteRes>,
    pub has_remote_content: bool,
    pub attachments_lazy: bool,
    pub is_focus: bool,
}

pub struct RemoteRes {
    pub url: String,
    pub label: String,
}

pub struct MoveOpt {
    pub value: String,
    pub label: String,
    pub selected: bool,
}

pub struct AttRow {
    pub id: String,
    pub filename: String,
    pub mime: String,
    pub size_label: String,
    pub previewable: bool,
}

/// Affichage lisible : `o` / `Ko` / `Mo` (séparateur décimal français).
fn format_size_fr(bytes: u64) -> String {
    if bytes < 1000 {
        return format!("{bytes} o");
    }
    if bytes < 100_000 {
        let ko = bytes as f64 / 1000.0;
        if (ko - ko.round()).abs() < 0.05 {
            return format!("{} Ko", ko.round() as u64);
        }
        return format!("{ko:.1} Ko").replace('.', ",");
    }
    let mo = bytes as f64 / 1_000_000.0;
    if mo < 10.0 {
        format!("{mo:.2} Mo").replace('.', ",")
    } else if mo < 100.0 {
        format!("{mo:.1} Mo").replace('.', ",")
    } else {
        format!("{} Mo", mo.round() as u64)
    }
}

#[cfg(test)]
mod size_label_tests {
    use super::format_size_fr;

    #[test]
    fn formats_bytes_ko_mo() {
        assert_eq!(format_size_fr(0), "0 o");
        assert_eq!(format_size_fr(677), "677 o");
        assert_eq!(format_size_fr(12_500), "12,5 Ko");
        assert_eq!(format_size_fr(216_248), "0,22 Mo");
        assert_eq!(format_size_fr(5_500_000), "5,50 Mo");
    }
}

fn is_previewable_attachment(filename: &str, mime: &str) -> bool {
    let mime_l = mime.trim().to_ascii_lowercase();
    if mime_l.starts_with("image/png")
        || mime_l.starts_with("image/jpeg")
        || mime_l.starts_with("image/jpg")
        || mime_l.starts_with("image/gif")
        || mime_l.starts_with("image/webp")
        || mime_l.starts_with("image/svg")
        || mime_l == "application/pdf"
        || mime_l.starts_with("text/plain")
    {
        return true;
    }
    let name = filename.trim().to_ascii_lowercase();
    name.ends_with(".png")
        || name.ends_with(".jpg")
        || name.ends_with(".jpeg")
        || name.ends_with(".gif")
        || name.ends_with(".webp")
        || name.ends_with(".svg")
        || name.ends_with(".pdf")
        || name.ends_with(".txt")
}

fn att_rows_from_meta(list: Vec<crate::cli::himalaya::AttachmentMeta>) -> (Vec<AttRow>, Vec<AttRow>) {
    let mut real = Vec::new();
    let mut accessory = Vec::new();
    for a in list {
        let is_acc = is_accessory_attachment(&a.filename, &a.mime, a.size);
        let previewable = !is_acc && is_previewable_attachment(&a.filename, &a.mime);
        let row = AttRow {
            id: a.id,
            filename: a.filename,
            mime: a.mime,
            size_label: format_size_fr(a.size),
            previewable,
        };
        if is_acc {
            accessory.push(row);
        } else {
            real.push(row);
        }
    }
    (real, accessory)
}

/// Alimente le cache du corps sans rendre de HTML. Utilisé au survol, via
/// le pool de fond, pour que le clic suivant soit servi depuis SQLite.
async fn prefetch_message_body(
    state: &AppState,
    mailbox: &str,
    id: &str,
    account: Option<&str>,
) -> axum::response::Response {
    {
        let cache = state.cache.lock().await;
        if cache.load_message(mailbox, id).ok().flatten().is_some() {
            return StatusCode::NO_CONTENT.into_response();
        }
    }
    if !state.himalaya_available {
        return StatusCode::NO_CONTENT.into_response();
    }
    let _permit = state.cli_bg_limit.acquire().await.ok();
    if let Ok(msg) = state.himalaya.read_message(mailbox, id, account).await {
        let cache = state.cache.lock().await;
        let _ = cache.save_message(mailbox, &msg);
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn message(
    State(state): State<Arc<AppState>>,
    Query(q): Query<MessageQuery>,
) -> impl IntoResponse {
    let name = q.mailbox;
    let focus_id = q.id;
    if q.prefetch.unwrap_or(0) == 1 {
        if Prefs::is_ntfy_key(&name) {
            return StatusCode::NO_CONTENT.into_response();
        }
        let account = if let Some(a) = q.account.filter(|s| !s.is_empty()) {
            Some(a)
        } else {
            state.account().await
        };
        return prefetch_message_body(&state, &name, &focus_id, account.as_deref()).await;
    }
    if Prefs::is_ntfy_key(&name) {
        let prefs_snap = state.prefs.lock().await.clone();
        return ntfy_message_view(&state, &prefs_snap, &name, &focus_id, true).await;
    }
    let account = if let Some(a) = q.account.filter(|s| !s.is_empty()) {
        Some(a)
    } else {
        state.account().await
    };
    let account_ref = account.as_deref();
    let prefs_snap = state.prefs.lock().await.clone();

    let mut thread_ids: Vec<String> = parse_thread_ids(q.thread.as_deref().unwrap_or(""));
    if thread_ids.is_empty() || !prefs_snap.conversations {
        thread_ids = vec![focus_id.clone()];
    } else if !thread_ids.iter().any(|id| id == &focus_id) {
        thread_ids.push(focus_id.clone());
    }
    // Cap to keep UI responsive
    if thread_ids.len() > 25 {
        if let Some(pos) = thread_ids.iter().position(|id| id == &focus_id) {
            let start = pos.saturating_sub(12);
            let end = (start + 25).min(thread_ids.len());
            thread_ids = thread_ids[start..end].to_vec();
        } else {
            thread_ids.truncate(25);
        }
    }

    let thread_count_hint = thread_ids.len() as u32;

    // Mailboxes depuis le cache (instantané) — évite un `mailbox list` à chaque ouverture.
    let default_move = account
        .as_deref()
        .and_then(|a| prefs_snap.default_move_for(a).map(str::to_string));
    let mailboxes: Vec<MoveOpt> = {
        let cache = state.cache.lock().await;
        let cached = cache.load_mailboxes().unwrap_or_default();
        let acct_label = account
            .as_deref()
            .map(|a| prefs_snap.account_label(a))
            .unwrap_or_default();
        if !cached.is_empty() {
            cached
                .into_iter()
                .map(|m| {
                    let selected = default_move
                        .as_deref()
                        .map(|d| d.eq_ignore_ascii_case(&m.name))
                        .unwrap_or(false);
                    let label = if acct_label.is_empty() {
                        m.name.clone()
                    } else {
                        format!("{acct_label} · {}", m.name)
                    };
                    MoveOpt {
                        value: m.name,
                        label,
                        selected,
                    }
                })
                .collect()
        } else {
            drop(cache);
            if state.himalaya_available {
                let _permit = state.cli_limit.acquire().await.ok();
                match state.himalaya.list_mailboxes(account_ref).await {
                    Ok(boxes) => boxes
                        .into_iter()
                        .map(|m| {
                            let selected = default_move
                                .as_deref()
                                .map(|d| d.eq_ignore_ascii_case(&m.name))
                                .unwrap_or(false);
                            let label = if acct_label.is_empty() {
                                m.name.clone()
                            } else {
                                format!("{acct_label} · {}", m.name)
                            };
                            MoveOpt {
                                value: m.name,
                                label,
                                selected,
                            }
                        })
                        .collect(),
                    Err(_) => vec![],
                }
            } else {
                vec![]
            }
        }
    };

    // Cache d'abord : un prefetch au survol (ou une ouverture précédente)
    // évite d'attendre Himalaya au clic. Un rafraîchissement part en fond.
    let cached = {
        let cache = state.cache.lock().await;
        cache.load_message(&name, &focus_id).ok().flatten()
    };

    let mut loaded: Vec<(String, Option<crate::cli::himalaya::MessageView>, bool, Option<String>)> =
        Vec::with_capacity(thread_ids.len());
    let mut offline_any = false;
    let first_err: Option<String>;

    let (msg, offline, error) = if let Some(msg) = cached {
        if state.himalaya_available {
            let himalaya = state.himalaya.clone();
            let mb = name.clone();
            let mid = focus_id.clone();
            let acc = account.clone();
            let cache = Arc::clone(&state.cache);
            let bg_limit = Arc::clone(&state.cli_bg_limit);
            tokio::spawn(async move {
                let _permit = bg_limit.acquire().await.ok();
                if let Ok(fresh) = himalaya.read_message(&mb, &mid, acc.as_deref()).await {
                    let cache = cache.lock().await;
                    let _ = cache.save_message(&mb, &fresh);
                }
            });
        }
        (Some(msg), false, None)
    } else if state.himalaya_available {
        let _permit = state.cli_limit.acquire().await.ok();
        match state
            .himalaya
            .read_message(&name, &focus_id, account_ref)
            .await
        {
            Ok(msg) => {
                let cache = state.cache.lock().await;
                let _ = cache.save_message(&name, &msg);
                (Some(msg), false, None)
            }
            Err(e) => {
                let cache = state.cache.lock().await;
                (
                    cache.load_message(&name, &focus_id).ok().flatten(),
                    true,
                    Some(e.to_string()),
                )
            }
        }
    } else {
        let cache = state.cache.lock().await;
        (
            cache.load_message(&name, &focus_id).ok().flatten(),
            true,
            None,
        )
    };
    if offline {
        offline_any = true;
    }
    first_err = error;
    loaded.push((focus_id.clone(), msg, offline, first_err.clone()));

    if prefs_snap.conversations && thread_ids.len() > 1 {
        let cache = state.cache.lock().await;
        for tid in &thread_ids {
            if tid == &focus_id {
                continue;
            }
            if let Ok(Some(msg)) = cache.load_message(&name, tid) {
                loaded.push((tid.clone(), Some(msg), true, None));
            }
        }
    }

    // Ordre chrono du fil : respecter thread_ids
    loaded.sort_by_key(|(tid, _, _, _)| {
        thread_ids
            .iter()
            .position(|id| id == tid)
            .unwrap_or(usize::MAX)
    });

    if loaded.iter().all(|(_, m, _, _)| m.is_none()) {
        return Html(format!(
            r#"<div class="empty-read"><i data-lucide="mail-x"></i><p>Message introuvable{}</p></div>
               <script>lucide.createIcons()</script>"#,
            first_err
                .map(|e| format!(" ({e})"))
                .unwrap_or_default()
        ))
        .into_response();
    }

    let mut marked_read = false;
    let mut thread_parts: Vec<ThreadPart> = Vec::new();
    let mut thread_subject = String::new();

    for (tid, msg_opt, _off, _err) in loaded {
        let Some(msg) = msg_opt else {
            continue;
        };
        let was_unread = !msg.flags.iter().any(|f| {
            let x = f.to_ascii_lowercase();
            x == "seen" || x == "\\seen"
        });
        if was_unread && state.himalaya_available && !offline_any && tid == focus_id {
            let himalaya = state.himalaya.clone();
            let mb = name.clone();
            let mid = tid.clone();
            let acc = account.clone();
            let bg_limit = Arc::clone(&state.cli_bg_limit);
            tokio::spawn(async move {
                // Marquage différé : compte dans le pool de fond, pas dans
                // celui des actions utilisateur.
                let _permit = bg_limit.acquire().await.ok();
                let _ = himalaya
                    .set_flag(&mb, &mid, "seen", true, acc.as_deref())
                    .await;
            });
            marked_read = true;
        }
        let unread = was_unread && !(tid == focus_id && marked_read);

        let (body, has_remote_content, remote_resources) = if !msg.body_html.is_empty() {
            let s = sanitize_html(&msg.body_html);
            let (html, had_cid) = rewrite_cid_images(
                &s.html,
                &msg.cid_map,
                &name,
                &tid,
                account_ref,
            );
            let remotes: Vec<RemoteRes> = s
                .remote_urls
                .into_iter()
                .map(|url| RemoteRes {
                    label: remote_url_label(&url),
                    url,
                })
                .collect();
            (html, s.has_remote_content || had_cid || !remotes.is_empty(), remotes)
        } else if !msg.body_text.is_empty() {
            (plain_to_html(&msg.body_text), false, vec![])
        } else if !msg.raw_preview.is_empty() {
            (plain_to_html(&msg.raw_preview), false, vec![])
        } else {
            ("<p class=\"muted\">(corps vide)</p>".into(), false, vec![])
        };

        let (attachments, accessory_attachments) = att_rows_from_meta(msg.attachments);
        let attachments_lazy = state.himalaya_available && !offline_any && tid == focus_id;

        if thread_subject.is_empty() {
            thread_subject = msg.subject.clone();
        }
        if tid == focus_id {
            thread_subject = msg.subject.clone();
        }

        thread_parts.push(ThreadPart {
            is_focus: tid == focus_id,
            id: msg.id,
            subject: msg.subject,
            from_initial: msg
                .from
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string(),
            from: msg.from,
            to: msg.to,
            cc: msg.cc,
            date: msg.date,
            unread,
            body,
            attachments,
            accessory_attachments,
            remote_resources,
            has_remote_content,
            attachments_lazy,
        });
    }

    if thread_parts.is_empty() {
        return Html(
            r#"<div class="empty-read"><i data-lucide="mail-x"></i><p>Message introuvable</p></div>
               <script>lucide.createIcons()</script>"#
                .to_string(),
        )
        .into_response();
    }

    // Display subject without cascading Re:
    let mut display_subject = thread_subject.trim().to_string();
    loop {
        let before = display_subject.clone();
        for p in ["Re:", "RE:", "Fwd:", "FWD:", "Fw:", "Aw:", "SV:"] {
            if let Some(rest) = display_subject.strip_prefix(p) {
                display_subject = rest.trim_start().to_string();
            }
        }
        if display_subject == before {
            break;
        }
    }
    if display_subject.is_empty() {
        display_subject = thread_subject;
    }

    let thread_count = thread_count_hint.max(thread_parts.len() as u32);
    render(MessageTemplate {
        mailbox_enc: urlencoding::encode(&name).into_owned(),
        mailbox: name,
        id: focus_id,
        subject: display_subject,
        mailboxes,
        offline: offline_any,
        error: first_err,
        marked_read,
        thread_count,
        thread: thread_parts,
        account_enc: urlencoding::encode(account.as_deref().unwrap_or("")).into_owned(),
        account: account.unwrap_or_default(),
    })
}

#[derive(Deserialize)]
struct MessageAttachmentsQuery {
    pub mailbox: String,
    pub message_id: String,
    pub account: Option<String>,
}

#[derive(Template)]
#[template(path = "message_attachments.html")]
struct MessageAttachmentsTemplate {
    pub mailbox_enc: String,
    pub account_enc: String,
    pub message_id: String,
    pub attachments: Vec<AttRow>,
    pub accessory_attachments: Vec<AttRow>,
}

async fn message_attachments(
    State(state): State<Arc<AppState>>,
    Query(q): Query<MessageAttachmentsQuery>,
) -> impl IntoResponse {
    let account = q.account.filter(|s| !s.is_empty());
    let account_ref = account.as_deref();
    if !state.himalaya_available {
        return Html("<!-- no himalaya -->").into_response();
    }
    let _permit = state.cli_limit.acquire().await.ok();
    let list = match state
        .himalaya
        .list_attachments(&q.mailbox, &q.message_id, account_ref)
        .await
    {
        Ok(list) => list,
        Err(_) => return Html("<!-- att list failed -->").into_response(),
    };
    let (attachments, accessory_attachments) = att_rows_from_meta(list);

    let html = MessageAttachmentsTemplate {
        mailbox_enc: urlencoding::encode(&q.mailbox).into_owned(),
        account_enc: urlencoding::encode(account.as_deref().unwrap_or("")).into_owned(),
        message_id: q.message_id,
        attachments,
        accessory_attachments,
    }
    .render()
    .unwrap_or_else(|e| format!("<!-- att error: {e} -->"));
    Html(html).into_response()
}

#[derive(Deserialize)]
pub struct FlagForm {
    pub mailbox: String,
    pub id: String,
    pub seen: Option<String>,
    pub account: Option<String>,
    pub thread: Option<String>,
    /// Si présent : ne pas renvoyer le message (ex. action depuis la liste / clic droit)
    pub quiet: Option<String>,
}

async fn flag(
    State(state): State<Arc<AppState>>,
    Form(form): Form<FlagForm>,
) -> impl IntoResponse {
    let add = form.seen.as_deref() == Some("1") || form.seen.as_deref() == Some("true");
    let quiet =
        form.quiet.as_deref() == Some("1") || form.quiet.as_deref() == Some("true");

    if Prefs::is_ntfy_key(&form.mailbox) {
        {
            let mut prefs = state.prefs.lock().await;
            prefs.set_ntfy_read(&form.id, add);
            let _ = prefs.save();
        }
        if quiet {
            let id_js = serde_json::to_string(&form.id).unwrap_or_else(|_| "\"\"".into());
            let acc_js = serde_json::to_string(&form.mailbox).unwrap_or_else(|_| "\"\"".into());
            let seen_js = if add { "true" } else { "false" };
            return Html(format!(
                r##"<script>
if (window.HimaWeb) {{
  window.HimaWeb.applyEnvelopeSeen({id_js}, {acc_js}, {seen_js});
  window.HimaWeb.applyEnvelopeSeen({id_js}, "", {seen_js});
  window.HimaWeb.pollUnread();
}}
</script>"##
            ))
            .into_response();
        }
        let prefs_snap = state.prefs.lock().await.clone();
        return ntfy_message_view(&state, &prefs_snap, &form.mailbox, &form.id, false).await;
    }

    let account = form
        .account
        .filter(|s| !s.is_empty())
        .or(state.account().await);
    let _permit = state.cli_limit.acquire().await.ok();
    match state
        .himalaya
        .set_flag(
            &form.mailbox,
            &form.id,
            "seen",
            add,
            account.as_deref(),
        )
        .await
    {
        Ok(()) => {
            if quiet {
                let id_js = serde_json::to_string(&form.id).unwrap_or_else(|_| "\"\"".into());
                let acc_js = serde_json::to_string(account.as_deref().unwrap_or(""))
                    .unwrap_or_else(|_| "\"\"".into());
                let seen_js = if add { "true" } else { "false" };
                return Html(format!(
                    r##"<script>
if (window.HimaWeb) {{
  window.HimaWeb.applyEnvelopeSeen({id_js}, {acc_js}, {seen_js});
  window.HimaWeb.pollUnread();
}}
</script>"##
                ))
                .into_response();
            }
            let mut url = format!(
                "/partials/message?mailbox={}&id={}",
                urlencoding::encode(&form.mailbox),
                urlencoding::encode(&form.id)
            );
            if let Some(a) = account.as_deref() {
                url.push_str(&format!("&account={}", urlencoding::encode(a)));
            }
            if let Some(t) = form.thread.as_deref().filter(|s| !s.is_empty()) {
                let ids = parse_thread_ids(t);
                if ids.len() > 1 {
                    let enc = ids
                        .iter()
                        .map(|id| urlencoding::encode(id).into_owned())
                        .collect::<Vec<_>>()
                        .join(",");
                    url.push_str(&format!("&thread={enc}"));
                }
            }
            Redirect::to(&url).into_response()
        }
        Err(e) => Html(format!(r#"<div class="error">{e}</div>"#)).into_response(),
    }
}

#[derive(Deserialize)]
pub struct MoveForm {
    pub mailbox: String,
    pub id: String,
    pub to: String,
    pub account: Option<String>,
    pub to_account: Option<String>,
}

async fn move_msg(
    State(state): State<Arc<AppState>>,
    Form(form): Form<MoveForm>,
) -> impl IntoResponse {
    if Prefs::is_ntfy_key(&form.mailbox)
        || form
            .account
            .as_deref()
            .map(Prefs::is_ntfy_key)
            .unwrap_or(false)
    {
        return Html(
            r#"<div class="error">Les notifications NTFY ne se déplacent pas vers IMAP — utilisez Transférer / Répondre, ou Supprimer (masquage local).</div>"#
                .to_string(),
        )
        .into_response();
    }
    let _permit = state.cli_limit.acquire().await.ok();
    let account = form
        .account
        .filter(|s| !s.is_empty())
        .or(state.account().await);
    let to_account = form
        .to_account
        .filter(|s| !s.is_empty())
        .or_else(|| account.clone());
    match state
        .himalaya
        .move_message_to_account(
            &form.mailbox,
            &form.to,
            &form.id,
            account.as_deref(),
            to_account.as_deref(),
        )
        .await
    {
        Ok(()) => {
            let id_js = serde_json::to_string(&form.id).unwrap_or_else(|_| "\"\"".into());
            let mb_js = serde_json::to_string(&form.mailbox).unwrap_or_else(|_| "\"\"".into());
            let acc = account.as_deref().unwrap_or("");
            let acc_js = serde_json::to_string(acc).unwrap_or_else(|_| "\"\"".into());
            let to_js = serde_json::to_string(&form.to).unwrap_or_else(|_| "\"\"".into());
            Html(format!(
                r##"<div class="empty-read" data-mail-event="moved" data-id="{id}" data-mailbox="{mb}" data-account="{acc}" data-to="{to}">
  <i data-lucide="folder-input"></i>
  <p class="muted">Déplacé vers {to_label}</p>
</div>
<script>
if (window.HimaWeb) {{
  window.HimaWeb.onMessageMoved({{ id: {id_js}, mailbox: {mb_js}, account: {acc_js}, to: {to_js} }});
}}
if (window.lucide) lucide.createIcons();
</script>"##,
                id = html_escape(&form.id),
                mb = html_escape(&form.mailbox),
                acc = html_escape(acc),
                to = html_escape(&form.to),
                to_label = html_escape(&form.to),
                id_js = id_js,
                mb_js = mb_js,
                acc_js = acc_js,
                to_js = to_js,
            ))
            .into_response()
        }
        Err(e) => Html(format!(r#"<div class="error">{e}</div>"#)).into_response(),
    }
}

#[derive(Deserialize)]
pub struct MoveApiItem {
    pub id: String,
    pub mailbox: String,
    pub account: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
}

#[derive(Deserialize)]
pub struct MoveApiBody {
    pub items: Vec<MoveApiItem>,
    pub to_mailbox: String,
    pub to_account: Option<String>,
}

#[derive(Serialize)]
struct MoveApiResult {
    ok: bool,
    moved: Vec<MoveApiMoved>,
    errors: Vec<String>,
}

#[derive(Serialize)]
struct MoveApiMoved {
    id: String,
    mailbox: String,
    account: String,
    message_id: String,
    to_mailbox: String,
    permanent: bool,
}

fn resolve_trash_mailbox(account: &str) -> String {
    crate::accounts_config::get_mailbox_alias(account, "trash")
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Trash".into())
}

fn mailbox_is_trash(mailbox: &str, trash: &str) -> bool {
    mailbox.eq_ignore_ascii_case(trash)
        || mailbox.eq_ignore_ascii_case("Trash")
        || mailbox.eq_ignore_ascii_case("Corbeille")
        || mailbox.to_ascii_lowercase().ends_with("/trash")
        || mailbox.to_ascii_lowercase().ends_with(".trash")
}

async fn lookup_message_id(
    state: &AppState,
    mailbox: &str,
    id: &str,
    account: Option<&str>,
    hinted: Option<&str>,
) -> String {
    if let Some(h) = hinted.map(str::trim).filter(|s| !s.is_empty()) {
        return h.to_string();
    }
    match state
        .himalaya
        .list_envelopes(mailbox, 1, 200, account)
        .await
    {
        Ok(list) => list
            .into_iter()
            .find(|e| e.id == id)
            .map(|e| e.message_id)
            .unwrap_or_default(),
        Err(_) => String::new(),
    }
}

async fn find_uid_by_message_id(
    state: &AppState,
    mailbox: &str,
    message_id: &str,
    account: Option<&str>,
) -> Option<String> {
    let mid = message_id.trim();
    if mid.is_empty() {
        return None;
    }
    let normalize = |s: &str| {
        s.trim()
            .trim_matches(|c| c == '<' || c == '>')
            .to_ascii_lowercase()
    };
    let target = normalize(mid);
    let list = state
        .himalaya
        .list_envelopes(mailbox, 1, 200, account)
        .await
        .ok()?;
    list.into_iter()
        .find(|e| !e.message_id.is_empty() && normalize(&e.message_id) == target)
        .map(|e| e.id)
}

async fn move_api(
    State(state): State<Arc<AppState>>,
    Json(body): Json<MoveApiBody>,
) -> impl IntoResponse {
    let _permit = state.cli_limit.acquire().await.ok();
    let to_mailbox = body.to_mailbox.trim().to_string();
    if to_mailbox.is_empty() || body.items.is_empty() {
        return Json(MoveApiResult {
            ok: false,
            moved: vec![],
            errors: vec!["destination ou liste vide".into()],
        })
        .into_response();
    }
    let default_acc = state.account().await;
    let to_account_opt = body.to_account.filter(|s| !s.is_empty());
    let mut moved = Vec::new();
    let mut errors = Vec::new();
    for item in body.items {
        let from_acc = item
            .account
            .filter(|s| !s.is_empty())
            .or_else(|| default_acc.clone());
        let to_acc = to_account_opt.clone().or_else(|| from_acc.clone());
        let message_id = lookup_message_id(
            &state,
            &item.mailbox,
            &item.id,
            from_acc.as_deref(),
            item.message_id.as_deref(),
        )
        .await;
        match state
            .himalaya
            .move_message_to_account(
                &item.mailbox,
                &to_mailbox,
                &item.id,
                from_acc.as_deref(),
                to_acc.as_deref(),
            )
            .await
        {
            Ok(()) => moved.push(MoveApiMoved {
                id: item.id,
                mailbox: item.mailbox,
                account: from_acc.unwrap_or_default(),
                message_id,
                to_mailbox: to_mailbox.clone(),
                permanent: false,
            }),
            Err(e) => errors.push(format!("{}: {e}", item.id)),
        }
    }
    Json(MoveApiResult {
        ok: errors.is_empty(),
        moved,
        errors,
    })
    .into_response()
}

#[derive(Deserialize)]
pub struct DeleteApiBody {
    pub items: Vec<MoveApiItem>,
}

#[derive(Serialize)]
struct DeleteApiResult {
    ok: bool,
    deleted: Vec<MoveApiMoved>,
    errors: Vec<String>,
}

async fn delete_api(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DeleteApiBody>,
) -> impl IntoResponse {
    if body.items.is_empty() {
        return Json(DeleteApiResult {
            ok: false,
            deleted: vec![],
            errors: vec!["liste vide".into()],
        })
        .into_response();
    }
    let default_acc = state.account().await;
    let mut deleted = Vec::new();
    let mut errors = Vec::new();
    for item in body.items {
        let account = item
            .account
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| default_acc.clone());
        let acc_name = account.clone().unwrap_or_default();

        // NTFY : masquage local (pas d’IMAP)
        if Prefs::is_ntfy_key(&item.mailbox) || Prefs::is_ntfy_key(&acc_name) {
            delete_ntfy_local(&state, &item.id).await;
            let mb = item.mailbox.clone();
            deleted.push(MoveApiMoved {
                id: item.id,
                mailbox: mb.clone(),
                account: if Prefs::is_ntfy_key(&acc_name) {
                    acc_name
                } else {
                    mb
                },
                message_id: String::new(),
                to_mailbox: String::new(),
                permanent: true,
            });
            continue;
        }

        let _permit = state.cli_limit.acquire().await.ok();
        let trash = if acc_name.is_empty() {
            "Trash".into()
        } else {
            resolve_trash_mailbox(&acc_name)
        };
        let permanent = mailbox_is_trash(&item.mailbox, &trash);
        let message_id = lookup_message_id(
            &state,
            &item.mailbox,
            &item.id,
            account.as_deref(),
            item.message_id.as_deref(),
        )
        .await;
        let to_mailbox = if permanent {
            String::new()
        } else {
            trash
        };
        match state
            .himalaya
            .delete_message(&item.mailbox, &item.id, account.as_deref())
            .await
        {
            Ok(()) => deleted.push(MoveApiMoved {
                id: item.id,
                mailbox: item.mailbox,
                account: acc_name,
                message_id,
                to_mailbox,
                permanent,
            }),
            Err(e) => errors.push(format!("{}: {e}", item.id)),
        }
    }
    Json(DeleteApiResult {
        ok: errors.is_empty(),
        deleted,
        errors,
    })
    .into_response()
}

#[derive(Deserialize)]
pub struct UndoApiItem {
    pub account: String,
    pub message_id: String,
    pub from_mailbox: String,
    pub to_mailbox: String,
}

#[derive(Deserialize)]
pub struct UndoApiBody {
    pub items: Vec<UndoApiItem>,
}

#[derive(Serialize)]
struct UndoApiResult {
    ok: bool,
    restored: usize,
    errors: Vec<String>,
}

async fn undo_api(
    State(state): State<Arc<AppState>>,
    Json(body): Json<UndoApiBody>,
) -> impl IntoResponse {
    if body.items.is_empty() {
        return Json(UndoApiResult {
            ok: false,
            restored: 0,
            errors: vec!["rien à annuler".into()],
        })
        .into_response();
    }
    let mut restored = 0usize;
    let mut errors = Vec::new();
    for item in body.items {
        let mid = item.message_id.trim();
        let to_mb = item.to_mailbox.trim();
        let from_mb = item.from_mailbox.trim();
        if mid.is_empty() || to_mb.is_empty() || from_mb.is_empty() {
            errors.push("entrée undo incomplète (message-id / dossiers)".into());
            continue;
        }
        let _permit = state.cli_limit.acquire().await.ok();
        let account = if item.account.trim().is_empty() {
            None
        } else {
            Some(item.account.trim().to_string())
        };
        let Some(uid) =
            find_uid_by_message_id(&state, to_mb, mid, account.as_deref()).await
        else {
            errors.push(format!(
                "message introuvable dans « {to_mb} » (Message-ID manquant ou déjà déplacé)"
            ));
            continue;
        };
        match state
            .himalaya
            .move_message_to_account(
                to_mb,
                from_mb,
                &uid,
                account.as_deref(),
                account.as_deref(),
            )
            .await
        {
            Ok(()) => restored += 1,
            Err(e) => errors.push(format!("{uid}: {e}")),
        }
    }
    Json(UndoApiResult {
        ok: errors.is_empty() && restored > 0,
        restored,
        errors,
    })
    .into_response()
}

#[derive(Deserialize)]
pub struct DeleteForm {
    pub mailbox: String,
    pub id: String,
    pub account: Option<String>,
}

async fn delete_ntfy_local(state: &AppState, id: &str) {
    let mut prefs = state.prefs.lock().await;
    prefs.set_ntfy_deleted(id, true);
    let _ = prefs.save();
}

fn ntfy_delete_ok_html(id: &str, mailbox: &str, account: &str) -> axum::response::Response {
    let id_js = serde_json::to_string(id).unwrap_or_else(|_| "\"\"".into());
    let mb_js = serde_json::to_string(mailbox).unwrap_or_else(|_| "\"\"".into());
    let acc_js = serde_json::to_string(account).unwrap_or_else(|_| "\"\"".into());
    Html(format!(
        r##"<div class="empty-read" data-mail-event="deleted" data-id="{}" data-mailbox="{}" data-account="{}">
  <i data-lucide="mail-open"></i><p class="muted">Notification masquée (local)</p>
</div>
<script>
if (window.HimaWeb) {{
  window.HimaWeb.onMessageDeleted({{ id: {id_js}, mailbox: {mb_js}, account: {acc_js} }});
  window.HimaWeb.pollUnread && window.HimaWeb.pollUnread();
}}
if (window.lucide) lucide.createIcons();
</script>"##,
        html_escape(id),
        html_escape(mailbox),
        html_escape(account),
    ))
    .into_response()
}

async fn delete_msg(
    State(state): State<Arc<AppState>>,
    Form(form): Form<DeleteForm>,
) -> impl IntoResponse {
    if Prefs::is_ntfy_key(&form.mailbox)
        || form
            .account
            .as_deref()
            .map(Prefs::is_ntfy_key)
            .unwrap_or(false)
    {
        delete_ntfy_local(&state, &form.id).await;
        let acc = form
            .account
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| form.mailbox.clone());
        return ntfy_delete_ok_html(&form.id, &form.mailbox, &acc);
    }

    let _permit = state.cli_limit.acquire().await.ok();
    let account = form
        .account
        .filter(|s| !s.is_empty())
        .or(state.account().await);
    match state
        .himalaya
        .delete_message(&form.mailbox, &form.id, account.as_deref())
        .await
    {
        Ok(()) => {
            let id_js = serde_json::to_string(&form.id).unwrap_or_else(|_| "\"\"".into());
            let mb_js = serde_json::to_string(&form.mailbox).unwrap_or_else(|_| "\"\"".into());
            let acc_js = serde_json::to_string(account.as_deref().unwrap_or(""))
                .unwrap_or_else(|_| "\"\"".into());
            Html(format!(
                r##"<div class="empty-read" data-mail-event="deleted" data-id="{}" data-mailbox="{}" data-account="{}">
  <i data-lucide="mail-open"></i><p class="muted">Message supprimé</p>
</div>
<script>
if (window.HimaWeb) {{
  window.HimaWeb.onMessageDeleted({{ id: {id_js}, mailbox: {mb_js}, account: {acc_js} }});
}}
if (window.lucide) lucide.createIcons();
</script>"##,
                html_escape(&form.id),
                html_escape(&form.mailbox),
                html_escape(account.as_deref().unwrap_or("")),
            ))
            .into_response()
        }
        Err(e) => Html(format!(r#"<div class="error">{e}</div>"#)).into_response(),
    }
}

#[derive(Deserialize)]
pub struct DeleteBatchForm {
    /// JSON : `[{"mailbox":"...","id":"...","account":"..."}, ...]`
    pub items: String,
}

async fn delete_batch(
    State(state): State<Arc<AppState>>,
    Form(form): Form<DeleteBatchForm>,
) -> impl IntoResponse {
    #[derive(Deserialize)]
    struct Item {
        mailbox: String,
        id: String,
        #[serde(default)]
        account: String,
    }
    let items: Vec<Item> = match serde_json::from_str(&form.items) {
        Ok(v) => v,
        Err(e) => {
            return Html(format!(r#"<div class="error">Sélection invalide: {e}</div>"#))
                .into_response();
        }
    };
    if items.is_empty() {
        return Html(r#"<div class="error">Aucun message sélectionné</div>"#.to_string())
            .into_response();
    }
    if items.len() > 50 {
        return Html(r#"<div class="error">Maximum 50 messages à la fois</div>"#.to_string())
            .into_response();
    }

    let default_account = state.account().await;
    let mut ok = 0u32;
    let mut errors: Vec<String> = Vec::new();
    let mut last_ok: Option<(String, String, String)> = None;

    for it in &items {
        if Prefs::is_ntfy_key(&it.mailbox) || Prefs::is_ntfy_key(&it.account) {
            delete_ntfy_local(&state, &it.id).await;
            ok += 1;
            last_ok = Some((
                it.id.clone(),
                it.mailbox.clone(),
                if it.account.is_empty() {
                    it.mailbox.clone()
                } else {
                    it.account.clone()
                },
            ));
            continue;
        }
        let _permit = state.cli_limit.acquire().await.ok();
        let account = if it.account.is_empty() {
            default_account.clone()
        } else {
            Some(it.account.clone())
        };
        match state
            .himalaya
            .delete_message(&it.mailbox, &it.id, account.as_deref())
            .await
        {
            Ok(()) => {
                ok += 1;
                last_ok = Some((
                    it.id.clone(),
                    it.mailbox.clone(),
                    account.unwrap_or_default(),
                ));
            }
            Err(e) => errors.push(format!("{}: {e}", it.id)),
        }
    }

    let err_html = if errors.is_empty() {
        String::new()
    } else {
        format!(
            r#"<div class="error">{} échec(s) : {}</div>"#,
            errors.len(),
            html_escape(&errors.join(" · "))
        )
    };

    let (id, mb, acc) = last_ok.unwrap_or_default();
    let id_js = serde_json::to_string(&id).unwrap_or_else(|_| "\"\"".into());
    let mb_js = serde_json::to_string(&mb).unwrap_or_else(|_| "\"\"".into());
    let acc_js = serde_json::to_string(&acc).unwrap_or_else(|_| "\"\"".into());
    let ids_js = serde_json::to_string(
        &items
            .iter()
            .map(|i| {
                serde_json::json!({
                    "id": i.id,
                    "account": i.account,
                    "mailbox": i.mailbox,
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".into());

    let items_attr = html_escape(&ids_js);
    Html(format!(
        r##"{err_html}<div class="empty-read" data-mail-event="deleted-batch" data-items="{items_attr}" data-id="{id_h}" data-mailbox="{mb_h}" data-account="{acc_h}"><i data-lucide="mail-open"></i><p class="muted">{ok} message(s) supprimé(s)</p></div>
<script>
if (window.HimaWeb) {{
  window.HimaWeb.onMessagesDeleted({{ items: {ids_js}, last: {{ id: {id_js}, mailbox: {mb_js}, account: {acc_js} }} }});
}}
if (window.lucide) lucide.createIcons();
</script>"##,
        err_html = err_html,
        items_attr = items_attr,
        id_h = html_escape(&id),
        mb_h = html_escape(&mb),
        acc_h = html_escape(&acc),
        ok = ok,
        ids_js = ids_js,
        id_js = id_js,
        mb_js = mb_js,
        acc_js = acc_js,
    ))
    .into_response()
}

#[derive(Deserialize)]
pub struct SelectAccountForm {
    pub account: String,
    pub mailbox: Option<String>,
}

async fn select_account(
    State(state): State<Arc<AppState>>,
    Form(form): Form<SelectAccountForm>,
) -> impl IntoResponse {
    {
        let mut prefs = state.prefs.lock().await;
        let acc = form.account.trim();
        prefs.account = if acc.is_empty() {
            None
        } else {
            Some(acc.to_string())
        };
        let _ = prefs.save();
    }
    let mb = form.mailbox.unwrap_or_else(|| "Inbox".into());
    Redirect::to(&format!("/?mailbox={}", urlencoding::encode(&mb))).into_response()
}

#[derive(serde::Serialize)]
struct UnreadFolder {
    key: String,
    label: String,
    account: String,
    mailbox: String,
    unread: u64,
    in_total: bool,
}

async fn unread_counts(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let prefs_snap = state.prefs.lock().await.clone();
    let notifications = prefs_snap.notifications;
    let mut folders = Vec::new();

    if !state.himalaya_available {
        return axum::Json(serde_json::json!({
            "notifications": notifications,
            "total": 0,
            "folders": []
        }))
        .into_response();
    }

    // Poll périodique : pool de fond, pour ne jamais retarder un clic.
    let infos = {
        let _permit = state.cli_bg_limit.acquire().await.ok();
        state.himalaya.list_accounts().await.unwrap_or_default()
    };
    let known: Vec<String> = infos.iter().map(|a| a.name.clone()).collect();
    let ordered = prefs_snap.ordered_accounts(&known);

    let accounts: Vec<Option<String>> = if prefs_snap.is_all_accounts() {
        ordered.into_iter().map(Some).collect()
    } else if let Some(a) = prefs_snap.selected_account() {
        vec![Some(a.to_string())]
    } else {
        vec![None]
    };

    // Un `list_mailboxes` par compte, en parallèle.
    let mut boxes_tasks = tokio::task::JoinSet::new();
    for acc in accounts {
        let st = Arc::clone(&state);
        boxes_tasks.spawn(async move {
            let _permit = st.cli_bg_limit.acquire().await.ok();
            let boxes = st.himalaya.list_mailboxes(acc.as_deref()).await;
            (acc, boxes)
        });
    }

    // Puis un `count_unseen` par dossier surveillé, en parallèle également.
    // C'est le point chaud : `count_unseen` est une recherche IMAP complète,
    // et il y en a un par dossier suivi de chaque compte.
    let mut count_tasks = tokio::task::JoinSet::new();
    while let Some(res) = boxes_tasks.join_next().await {
        let Ok((acc, Ok(boxes))) = res else {
            continue;
        };
        for m in boxes {
            let key = Prefs::folder_key(acc.as_deref(), &m.name);
            if !prefs_snap.is_watched(&key, &m.name) {
                continue;
            }
            let st = Arc::clone(&state);
            let acc = acc.clone();
            let prefs_snap = prefs_snap.clone();
            count_tasks.spawn(async move {
                let acc_ref = acc.as_deref();
                // Himalaya renvoie souvent unread: null → compter via recherche
                // uniquement quand le compteur n'était pas déjà fourni.
                let unread = if let Some(n) = m.unread {
                    n
                } else {
                    let _permit = st.cli_bg_limit.acquire().await.ok();
                    match st.himalaya.count_unseen(&m.name, acc_ref).await {
                        Ok(n) => n,
                        Err(_) => 0,
                    }
                };
                if unread == 0 {
                    return None;
                }
                let in_total = prefs_snap.contributes_to_unread_total(&key, &m.name);
                Some(UnreadFolder {
                    label: if let Some(a) = acc_ref {
                        format!("{a} / {}", mailbox_label(&m.name))
                    } else {
                        mailbox_label(&m.name)
                    },
                    account: acc_ref.unwrap_or("").to_string(),
                    mailbox: m.name.clone(),
                    unread,
                    key,
                    in_total,
                })
            });
        }
    }

    while let Some(res) = count_tasks.join_next().await {
        if let Ok(Some(folder)) = res {
            folders.push(folder);
        }
    }
    // L'ordre d'un JoinSet n'est pas déterministe : stabiliser l'affichage.
    folders.sort_by(|a, b| a.account.cmp(&b.account).then_with(|| a.mailbox.cmp(&b.mailbox)));

    // Compteurs NTFY surveillés
    for key in prefs_snap.ntfy_order_keys() {
        let folder_key = Prefs::ntfy_folder_key(&key);
        if !prefs_snap.is_watched(&folder_key, &key) {
            continue;
        }
        let unread = ntfy_unread_count(&prefs_snap, &key).await;
        if unread == 0 {
            continue;
        }
        folders.push(UnreadFolder {
            label: prefs_snap.account_label(&key),
            account: String::new(),
            mailbox: key.clone(),
            unread,
            key: folder_key,
            in_total: prefs_snap.contributes_to_unread_total(&Prefs::ntfy_folder_key(&key), &key),
        });
    }

    let total: u64 = folders
        .iter()
        .filter(|f| f.in_total)
        .map(|f| f.unread)
        .sum();

    // Plugin NTFY push : notifie seulement si le total augmente
    if prefs_snap.ntfy_sources.iter().any(|s| s.enabled && !s.topic.is_empty()) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static LAST_NTFY_TOTAL: AtomicU64 = AtomicU64::new(0);
        let prev = LAST_NTFY_TOTAL.load(Ordering::Relaxed);
        if total > prev {
            LAST_NTFY_TOTAL.store(total, Ordering::Relaxed);
            let title = format!("HimaWeb — {total} non-lu(s)");
            let body = folders
                .iter()
                .map(|f| format!("{}: {}", f.label, f.unread))
                .collect::<Vec<_>>()
                .join("\n");
            let targets: Vec<(String, String)> = prefs_snap
                .ntfy_sources
                .iter()
                .filter(|s| s.enabled && !s.topic.is_empty())
                .map(|s| (s.server.clone(), s.topic.clone()))
                .collect();
            tokio::spawn(async move {
                for (server, topic) in targets {
                    if let Err(e) = crate::plugins::ntfy_publish(&server, &topic, &title, &body).await
                    {
                        tracing::debug!("ntfy: {e}");
                    }
                }
            });
        } else if total < prev {
            LAST_NTFY_TOTAL.store(total, Ordering::Relaxed);
        }
    }

    axum::Json(serde_json::json!({
        "notifications": notifications,
        "total": total,
        "folders": folders,
        "mirador": prefs_snap.mirador_enabled,
    }))
    .into_response()
}

fn render(tpl: impl Template) -> axum::response::Response {
    match tpl.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => Html(format!("<pre>template error: {e}</pre>")).into_response(),
    }
}

fn ntfy_epoch_date(ts: i64) -> String {
    if ts <= 0 {
        return "—".into();
    }
    // Affichage simple UTC : YYYY-MM-DD HH:MM
    let secs = ts;
    let days = secs / 86400;
    let rem = secs % 86400;
    let hour = rem / 3600;
    let min = (rem % 3600) / 60;
    // Algorithme civil depuis jours Unix (1970-01-01)
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {hour:02}:{min:02}")
}

async fn ntfy_envelopes(
    prefs: &Prefs,
    mailbox_key: &str,
    query: &str,
    page: u32,
    page_size: u32,
    rows_only: bool,
    auto_pages: u32,
) -> axum::response::Response {
    let mut base_qs = format!("mailbox={}", urlencoding::encode(mailbox_key));
    if !query.is_empty() {
        base_qs.push_str(&format!("&q={}", urlencoding::encode(query)));
    }
    let sources = prefs.ntfy_sources_for_key(mailbox_key);
    if sources.is_empty() {
        return render(EnvelopesTemplate {
            mailbox: mailbox_key.into(),
            mailbox_enc: urlencoding::encode(mailbox_key).into_owned(),
            next_page: None,
            envelopes: vec![],
            offline: false,
            error: Some("ntfy non configuré (Plugins → NTFY)".into()),
            all_mode: false,
            query: query.to_string(),
            rows_only,
            stale: false,
            base_qs,
            next_auto_pages: auto_pages.saturating_add(1),
            auto_load: false,
        });
    }

    let q = query.trim().to_ascii_lowercase();
    let mut envelopes = Vec::new();
    let mut errs = Vec::new();
    let label = prefs.account_label(mailbox_key);
    let icon = prefs.account_icon(mailbox_key);
    let color = prefs.account_color(mailbox_key);

    for src in sources {
        match crate::plugins::ntfy_poll(&src.server, &src.topic).await {
            Ok(msgs) => {
                for m in msgs {
                    if !q.is_empty()
                        && !m.title.to_ascii_lowercase().contains(&q)
                        && !m.message.to_ascii_lowercase().contains(&q)
                        && !m.topic.to_ascii_lowercase().contains(&q)
                    {
                        continue;
                    }
                    let subject = if m.title.trim().is_empty() {
                        let preview: String = m.message.chars().take(80).collect();
                        if m.message.chars().count() > 80 {
                            format!("{preview}…")
                        } else if preview.is_empty() {
                            "(sans titre)".into()
                        } else {
                            preview
                        }
                    } else {
                        m.title.clone()
                    };
                    let from = if m.tags.is_empty() {
                        format!("ntfy/{}", m.topic)
                    } else {
                        format!("ntfy/{} · {}", m.topic, m.tags.join(","))
                    };
                    let date = ntfy_epoch_date(m.time);
                    // id composé source::msg pour retrouver le bon topic en lecture
                    let composite = format!("{}::{}", src.id, m.id);
                    if prefs.is_ntfy_deleted(&composite) {
                        continue;
                    }
                    let unread = !prefs.is_ntfy_read(&composite);
                    envelopes.push(EnvelopeRow {
                        id: composite.clone(),
                        subject,
                        from,
                        to: String::new(),
                        from_initial: "N".into(),
                        date: date.clone(),
                        date_short: date,
                        unread,
                        has_attachment: false,
                        account: mailbox_key.to_string(),
                        account_enc: urlencoding::encode(mailbox_key).into_owned(),
                        account_label: label.clone(),
                        account_icon: icon.clone(),
                        color: color.clone(),
                        thread_count: 1,
                        participants: String::new(),
                        thread_ids: composite.clone(),
                        thread_ids_enc: urlencoding::encode(&composite).into_owned(),
                        message_id: m.id,
                        id_enc: urlencoding::encode(&composite).into_owned(),
                    });
                }
            }
            Err(e) => errs.push(format!("{}: {e}", src.topic)),
        }
    }
    envelopes.sort_by(|a, b| b.date.cmp(&a.date));

    let total = envelopes.len();
    let start = ((page - 1) * page_size) as usize;
    let mut rows = if start < envelopes.len() {
        let end = (start + page_size as usize).min(envelopes.len());
        envelopes[start..end].to_vec()
    } else {
        vec![]
    };
    if page > 1 && rows.is_empty() {
        rows.clear();
    }
    let next_page = if ((page * page_size) as usize) < total {
        Some(page + 1)
    } else {
        None
    };
    let error = if errs.is_empty() {
        None
    } else {
        Some(errs.join(" · "))
    };

    render(EnvelopesTemplate {
        mailbox: mailbox_key.into(),
        mailbox_enc: urlencoding::encode(mailbox_key).into_owned(),
        next_page,
        envelopes: rows,
        offline: false,
        error,
        all_mode: false,
        query: query.to_string(),
        rows_only,
        stale: false,
        base_qs,
        next_auto_pages: auto_pages.saturating_add(1),
        auto_load: auto_pages < AUTO_PAGES_MAX,
    })
}

#[derive(Template)]
#[template(path = "ntfy_message.html")]
struct NtfyMessageTemplate {
    pub id: String,
    pub mailbox: String,
    pub subject: String,
    pub from: String,
    pub date: String,
    pub body: String,
    pub body_enc: String,
    pub subject_enc: String,
    pub unread: bool,
    pub marked_read: bool,
    pub error: Option<String>,
}

async fn ntfy_message_view(
    state: &AppState,
    prefs: &Prefs,
    mailbox_key: &str,
    id: &str,
    auto_mark_read: bool,
) -> axum::response::Response {
    let (src_id, msg_id) = id
        .split_once("::")
        .map(|(a, b)| (Some(a), b))
        .unwrap_or((None, id));

    let sources: Vec<crate::prefs::NtfySource> = if let Some(sid) = src_id {
        prefs
            .ntfy_sources
            .iter()
            .filter(|s| s.id == sid)
            .cloned()
            .collect()
    } else {
        prefs
            .ntfy_sources_for_key(mailbox_key)
            .into_iter()
            .cloned()
            .collect()
    };

    if sources.is_empty() {
        return render(NtfyMessageTemplate {
            id: id.to_string(),
            mailbox: mailbox_key.to_string(),
            subject: String::new(),
            from: String::new(),
            date: String::new(),
            body: String::new(),
            body_enc: String::new(),
            subject_enc: String::new(),
            unread: false,
            marked_read: false,
            error: Some("ntfy non configuré".into()),
        });
    }

    for src in &sources {
        match crate::plugins::ntfy_poll(&src.server, &src.topic).await {
            Ok(msgs) => {
                if let Some(m) = msgs.into_iter().find(|m| m.id == msg_id) {
                    let composite = format!("{}::{}", src.id, m.id);
                    if prefs.is_ntfy_deleted(&composite) {
                        return render(NtfyMessageTemplate {
                            id: composite,
                            mailbox: mailbox_key.to_string(),
                                            subject: String::new(),
                            from: String::new(),
                            date: String::new(),
                            body: String::new(),
                            body_enc: String::new(),
                            subject_enc: String::new(),
                            unread: false,
                            marked_read: false,
                            error: Some("Notification masquée.".into()),
                        });
                    }
                    let subject = if m.title.trim().is_empty() {
                        "(sans titre)".to_string()
                    } else {
                        m.title.clone()
                    };
                    let from = if m.tags.is_empty() {
                        format!("ntfy/{}", m.topic)
                    } else {
                        format!("ntfy/{} · {}", m.topic, m.tags.join(","))
                    };
                    let body_text = m.message.clone();
                    let quoted = format!(
                        "\n\n——— Notification ntfy ——\nTopic: {}\nDate: {}\n\n{}",
                        m.topic,
                        ntfy_epoch_date(m.time),
                        body_text
                    );
                    let was_unread = !prefs.is_ntfy_read(&composite);
                    let mut marked_read = false;
                    let mut unread = was_unread;
                    if auto_mark_read && was_unread {
                        let mut p = state.prefs.lock().await;
                        p.set_ntfy_read(&composite, true);
                        let _ = p.save();
                        marked_read = true;
                        unread = false;
                    }
                    return render(NtfyMessageTemplate {
                        id: composite,
                        mailbox: mailbox_key.to_string(),
                                    subject: subject.clone(),
                        from,
                        date: ntfy_epoch_date(m.time),
                        body: plain_to_html(&body_text),
                        body_enc: urlencoding::encode(&quoted).into_owned(),
                        subject_enc: urlencoding::encode(&format!("Re: {subject}")).into_owned(),
                        unread,
                        marked_read,
                        error: None,
                    });
                }
            }
            Err(e) => {
                return render(NtfyMessageTemplate {
                    id: id.to_string(),
                    mailbox: mailbox_key.to_string(),
                            subject: String::new(),
                    from: String::new(),
                    date: String::new(),
                    body: String::new(),
                    body_enc: String::new(),
                    subject_enc: String::new(),
                    unread: false,
                    marked_read: false,
                    error: Some(e),
                });
            }
        }
    }

    render(NtfyMessageTemplate {
        id: id.to_string(),
        mailbox: mailbox_key.to_string(),
        subject: String::new(),
        from: String::new(),
        date: String::new(),
        body: String::new(),
        body_enc: String::new(),
        subject_enc: String::new(),
        unread: false,
        marked_read: false,
        error: Some("Notification introuvable (expirée du cache ntfy ?)".into()),
    })
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
