use std::sync::Arc;

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;

use crate::cli::himalaya::Envelope;
use crate::prefs::Prefs;
use crate::sanitize::{plain_to_html, sanitize_html};
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/partials/sidebar", get(sidebar))
        .route("/partials/envelopes", get(envelopes))
        .route("/partials/message", get(message))
        .route("/partials/message/flag", post(flag))
        .route("/partials/message/move", post(move_msg))
        .route("/partials/message/delete", post(delete_msg))
        .route("/partials/message/delete-batch", post(delete_batch))
        .route("/account/select", post(select_account))
        .route("/api/mail/unread", get(unread_counts))
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
    pub selected_account: String,
    pub all_selected: bool,
    pub show_merged_inbox: bool,
    pub merged_inbox_active: bool,
    pub merged_inbox_unread: u64,
    pub pinned: Vec<MailboxRow>,
    pub mailboxes: Vec<MailboxRow>,
    pub more: Vec<MailboxRow>,
    pub current: String,
    pub offline: bool,
}

pub struct AccountRow {
    pub name: String,
    pub label: String,
    pub icon: String,
    pub is_default: bool,
    pub selected: bool,
    pub color: String,
}

pub struct MailboxRow {
    pub name: String,
    pub name_enc: String,
    pub label: String,
    pub unread: u64,
    pub icon: String,
    pub active: bool,
    pub pinned: bool,
    pub folder_key: String,
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
    if n == "inbox" || n.ends_with("/inbox") {
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

fn mailbox_label(name: &str) -> String {
    name.rsplit('/').next().unwrap_or(name).to_string()
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
    let _permit = state.cli_limit.acquire().await.ok();

    let account_infos = if state.himalaya_available {
        state.himalaya.list_accounts().await.unwrap_or_default()
    } else {
        vec![]
    };

    let known_names: Vec<String> = account_infos.iter().map(|a| a.name.clone()).collect();
    let ordered = prefs_snap.ordered_accounts(&known_names);

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
                    icon: prefs_snap.account_icon(&a.name),
                    is_default: a.is_default,
                    selected,
                    color: prefs_snap.account_color(&a.name),
                }
            })
        })
        .collect();

    let mut offline = false;
    let mut rows: Vec<MailboxRow> = Vec::new();

    if all_selected {
        // Un arbre par compte + couleur, pour distinguer l'appartenance
        let mut any_ok = false;
        for acc_name in &ordered {
            let color = prefs_snap.account_color(acc_name);
            let boxes = if state.himalaya_available {
                match state.himalaya.list_mailboxes(Some(acc_name)).await {
                    Ok(list) => {
                        any_ok = true;
                        list
                    }
                    Err(_) => {
                        offline = true;
                        vec![]
                    }
                }
            } else {
                vec![]
            };
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
                folder_key: String::new(),
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
                let mut unread = m.unread;
                if unread == 0 && prefs_snap.is_watched(&key, &m.name) && state.himalaya_available {
                    if let Ok(n) = state.himalaya.count_unseen(&m.name, Some(acc_name)).await {
                        unread = n;
                    }
                }
                rows.push(MailboxRow {
                    icon: mailbox_icon(&m.name).into(),
                    label: mailbox_label(&m.name),
                    name_enc: urlencoding::encode(&m.name).into_owned(),
                    pinned: prefs_snap.is_pinned(&key),
                    folder_key: key,
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
            let mut unread = m.unread;
            if unread == 0 && prefs_snap.is_watched(&key, &m.name) && state.himalaya_available {
                if let Ok(n) = state.himalaya.count_unseen(&m.name, account_ref).await {
                    unread = n;
                }
            }
            rows.push(MailboxRow {
                icon: mailbox_icon(&m.name).into(),
                label: mailbox_label(&m.name),
                name_enc: urlencoding::encode(&m.name).into_owned(),
                pinned: prefs_snap.is_pinned(&key),
                folder_key: key,
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
    }

    // Marquer les parents qui ont des enfants (via parent == tree_id)
    let parents_with_kids: std::collections::HashSet<String> =
        rows.iter().map(|r| r.parent.clone()).filter(|p| !p.is_empty()).collect();
    for r in &mut rows {
        r.has_children = parents_with_kids.contains(&r.tree_id);
    }

    if !all_selected {
        rows.sort_by(|a, b| {
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
        });
    } else {
        let order_idx = |name: &str| {
            ordered
                .iter()
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
        selected_account,
        all_selected,
        show_merged_inbox,
        merged_inbox_active,
        merged_inbox_unread,
        pinned,
        mailboxes,
        more: vec![],
        current,
        offline,
    })
}

#[derive(Deserialize)]
pub struct PageQuery {
    pub mailbox: Option<String>,
    pub page: Option<u32>,
    pub account: Option<String>,
    pub q: Option<String>,
    pub sort: Option<String>,
}

#[derive(Template)]
#[template(path = "envelopes.html")]
struct EnvelopesTemplate {
    pub mailbox: String,
    pub mailbox_enc: String,
    pub page: u32,
    pub prev_page: Option<u32>,
    pub next_page: Option<u32>,
    pub envelopes: Vec<EnvelopeRow>,
    pub offline: bool,
    pub error: Option<String>,
    pub all_mode: bool,
    pub query: String,
    pub query_enc: String,
    pub account_filter: String,
    pub account_filter_enc: String,
    pub sort: String,
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
            row.unread = true;
            row.from = list[u].from.clone();
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
    if date.len() >= 16 {
        let day = &date[0..10];
        let time = date.get(11..16).unwrap_or("");
        format!("{day} {time}")
    } else {
        date.to_string()
    }
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
    let page_size = 50u32;
    let query = q.q.unwrap_or_default();
    let sort = SortSpec::parse(q.sort.as_deref());
    let search_tokens = search_query_tokens(&query, false);
    let prefs_snap = state.prefs.lock().await.clone();
    let all_mode = prefs_snap.is_all_accounts();
    let account = prefs_snap.selected_account().map(str::to_string);
    let filter_account = q
        .account
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let _permit = state.cli_limit.acquire().await.ok();

    let (rows, offline, error) = if state.himalaya_available && all_mode {
        let infos = state.himalaya.list_accounts().await.unwrap_or_default();
        let known: Vec<String> = infos.iter().map(|a| a.name.clone()).collect();
        let ordered = prefs_snap.ordered_accounts(&known);
        let targets: Vec<String> = if let Some(fa) = &filter_account {
            if ordered.iter().any(|a| a == fa) {
                vec![fa.clone()]
            } else {
                ordered
            }
        } else {
            ordered
        };
        let mut merged = Vec::new();
        let mut errs = Vec::new();
        let mut offline_any = false;
        let mut seen = std::collections::HashSet::new();
        for acc in &targets {
            let color = prefs_snap.account_color(acc);
            match fetch_envelopes_for_account(
                &state,
                &name,
                page,
                page_size,
                Some(acc),
                &search_tokens,
                sort,
            )
            .await
            {
                Ok(list) => {
                    for row in envelope_rows(&list, acc, &prefs_snap) {
                        let key = format!("{}::{}", row.account, row.id);
                        if seen.insert(key) {
                            merged.push(row);
                        }
                    }
                }
                Err(e) => {
                    offline_any = true;
                    errs.push(format!("{acc}: {e}"));
                }
            }
        }
        sort_envelope_rows(&mut merged, sort);
        (
            merged,
            offline_any,
            if errs.is_empty() {
                None
            } else {
                Some(errs.join(" · "))
            },
        )
    } else if state.himalaya_available {
        let account_ref = account.as_deref();
        match fetch_envelopes_for_account(
            &state,
            &name,
            page,
            page_size,
            account_ref,
            &search_tokens,
            sort,
        )
        .await
        {
            Ok(list) => {
                if search_tokens.is_empty() && sort.is_default() {
                    let cache = state.cache.lock().await;
                    let _ = cache.save_envelopes(&name, &list);
                }
                (
                    envelope_rows(&list, account_ref.unwrap_or(""), &prefs_snap),
                    false,
                    None,
                )
            }
            Err(e) => {
                tracing::warn!("envelope list/search échoué: {e}");
                if search_tokens.is_empty() {
                    let cache = state.cache.lock().await;
                    let mut cached = cache.load_envelopes(&name).unwrap_or_default();
                    sort_envelopes(&mut cached, sort);
                    let start = ((page - 1) * page_size) as usize;
                    let slice: Vec<_> = cached
                        .into_iter()
                        .skip(start)
                        .take(page_size as usize)
                        .collect();
                    (
                        envelope_rows(&slice, account_ref.unwrap_or(""), &prefs_snap),
                        true,
                        Some(e.to_string()),
                    )
                } else {
                    (vec![], true, Some(e.to_string()))
                }
            }
        }
    } else {
        let cache = state.cache.lock().await;
        let mut cached = cache.load_envelopes(&name).unwrap_or_default();
        sort_envelopes(&mut cached, sort);
        let start = ((page - 1) * page_size) as usize;
        let slice: Vec<_> = cached
            .into_iter()
            .skip(start)
            .take(page_size as usize)
            .collect();
        (
            envelope_rows(&slice, account.as_deref().unwrap_or(""), &prefs_snap),
            true,
            None,
        )
    };

    let has_next = rows.len() as u32 >= page_size;
    let mailbox_enc = urlencoding::encode(&name).into_owned();
    let query_enc = urlencoding::encode(&query).into_owned();
    let account_filter = filter_account.clone().unwrap_or_default();
    let account_filter_enc = urlencoding::encode(&account_filter).into_owned();
    let sort_s = sort.as_str().to_string();
    render(EnvelopesTemplate {
        mailbox_enc,
        mailbox: name,
        page,
        prev_page: if page > 1 { Some(page - 1) } else { None },
        next_page: if has_next { Some(page + 1) } else { None },
        envelopes: rows,
        offline,
        error,
        all_mode,
        query,
        query_enc,
        account_filter,
        account_filter_enc,
        sort: sort_s,
    })
}

#[derive(Deserialize)]
pub struct MessageQuery {
    pub mailbox: String,
    pub id: String,
    pub account: Option<String>,
    /// IDs du fil (séparés par virgules), ordre chrono
    pub thread: Option<String>,
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
    pub is_focus: bool,
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
    pub size: u64,
}

async fn message(
    State(state): State<Arc<AppState>>,
    Query(q): Query<MessageQuery>,
) -> impl IntoResponse {
    let name = q.mailbox;
    let focus_id = q.id;
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
        // keep focus and neighbours around it
        if let Some(pos) = thread_ids.iter().position(|id| id == &focus_id) {
            let start = pos.saturating_sub(12);
            let end = (start + 25).min(thread_ids.len());
            thread_ids = thread_ids[start..end].to_vec();
        } else {
            thread_ids.truncate(25);
        }
    }

    let _permit = state.cli_limit.acquire().await.ok();

    let mut loaded: Vec<(String, Option<crate::cli::himalaya::MessageView>, bool, Option<String>)> =
        Vec::with_capacity(thread_ids.len());
    let mut offline_any = false;
    let mut first_err: Option<String> = None;

    for tid in &thread_ids {
        let (msg, offline, error) = if state.himalaya_available {
            match state.himalaya.read_message(&name, tid, account_ref).await {
                Ok(msg) => {
                    let cache = state.cache.lock().await;
                    let _ = cache.save_message(&name, &msg);
                    (Some(msg), false, None)
                }
                Err(e) => {
                    let cache = state.cache.lock().await;
                    (
                        cache.load_message(&name, tid).ok().flatten(),
                        true,
                        Some(e.to_string()),
                    )
                }
            }
        } else {
            let cache = state.cache.lock().await;
            (cache.load_message(&name, tid).ok().flatten(), true, None)
        };
        if offline {
            offline_any = true;
        }
        if first_err.is_none() {
            first_err = error.clone();
        }
        loaded.push((tid.clone(), msg, offline, error));
    }

    let default_move = account
        .as_deref()
        .and_then(|a| prefs_snap.default_move_for(a).map(str::to_string));

    let mailboxes: Vec<MoveOpt> = if state.himalaya_available {
        match state.himalaya.list_mailboxes(account_ref).await {
            Ok(boxes) => {
                let acct_label = account
                    .as_deref()
                    .map(|a| prefs_snap.account_label(a))
                    .unwrap_or_default();
                boxes
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
            }
            Err(_) => {
                let cache = state.cache.lock().await;
                cache
                    .load_mailboxes()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|m| MoveOpt {
                        selected: default_move
                            .as_deref()
                            .map(|d| d.eq_ignore_ascii_case(&m.name))
                            .unwrap_or(false),
                        label: m.name.clone(),
                        value: m.name,
                    })
                    .collect()
            }
        }
    } else {
        let cache = state.cache.lock().await;
        cache
            .load_mailboxes()
            .unwrap_or_default()
            .into_iter()
            .map(|m| MoveOpt {
                selected: false,
                label: m.name.clone(),
                value: m.name,
            })
            .collect()
    };

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
        if was_unread && state.himalaya_available && !offline_any {
            let himalaya = state.himalaya.clone();
            let mb = name.clone();
            let mid = tid.clone();
            let acc = account.clone();
            tokio::spawn(async move {
                let _ = himalaya
                    .set_flag(&mb, &mid, "seen", true, acc.as_deref())
                    .await;
            });
            if tid == focus_id {
                marked_read = true;
            }
        }
        let unread = was_unread && !(tid == focus_id && marked_read);

        let body = if !msg.body_html.is_empty() {
            sanitize_html(&msg.body_html)
        } else if !msg.body_text.is_empty() {
            plain_to_html(&msg.body_text)
        } else if !msg.raw_preview.is_empty() {
            plain_to_html(&msg.raw_preview)
        } else {
            "<p class=\"muted\">(corps vide)</p>".into()
        };

        let attachments = msg
            .attachments
            .into_iter()
            .map(|a| AttRow {
                id: a.id,
                filename: a.filename,
                mime: a.mime,
                size: a.size,
            })
            .collect();

        if thread_subject.is_empty() {
            thread_subject = msg.subject.clone();
        }
        // Prefer cleaned subject from focus
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

    let thread_count = thread_parts.len() as u32;
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
}

async fn move_msg(
    State(state): State<Arc<AppState>>,
    Form(form): Form<MoveForm>,
) -> impl IntoResponse {
    let _permit = state.cli_limit.acquire().await.ok();
    let account = form
        .account
        .filter(|s| !s.is_empty())
        .or(state.account().await);
    match state
        .himalaya
        .move_message(
            &form.mailbox,
            &form.to,
            &form.id,
            account.as_deref(),
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
pub struct DeleteForm {
    pub mailbox: String,
    pub id: String,
    pub account: Option<String>,
}

async fn delete_msg(
    State(state): State<Arc<AppState>>,
    Form(form): Form<DeleteForm>,
) -> impl IntoResponse {
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

    Html(format!(
        r##"{err_html}<div class="empty-read"><i data-lucide="mail-open"></i><p class="muted">{ok} message(s) supprimé(s)</p></div>
<script>
if (window.HimaWeb) {{
  window.HimaWeb.onMessagesDeleted({{ items: {ids_js}, last: {{ id: {id_js}, mailbox: {mb_js}, account: {acc_js} }} }});
}}
if (window.lucide) lucide.createIcons();
</script>"##
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

    let _permit = state.cli_limit.acquire().await.ok();
    let infos = state.himalaya.list_accounts().await.unwrap_or_default();
    let known: Vec<String> = infos.iter().map(|a| a.name.clone()).collect();
    let ordered = prefs_snap.ordered_accounts(&known);

    let accounts: Vec<Option<String>> = if prefs_snap.is_all_accounts() {
        ordered.into_iter().map(Some).collect()
    } else if let Some(a) = prefs_snap.selected_account() {
        vec![Some(a.to_string())]
    } else {
        vec![None]
    };

    for acc in accounts {
        let acc_ref = acc.as_deref();
        let Ok(boxes) = state.himalaya.list_mailboxes(acc_ref).await else {
            continue;
        };
        for m in boxes {
            let key = Prefs::folder_key(acc_ref, &m.name);
            if !prefs_snap.is_watched(&key, &m.name) {
                continue;
            }
            // Himalaya renvoie souvent unread: null → compter via recherche
            let unread = match state.himalaya.count_unseen(&m.name, acc_ref).await {
                Ok(n) => n,
                Err(_) => m.unread,
            };
            if unread == 0 {
                continue;
            }
            folders.push(UnreadFolder {
                label: if let Some(a) = acc_ref {
                    format!("{a} / {}", mailbox_label(&m.name))
                } else {
                    mailbox_label(&m.name)
                },
                account: acc_ref.unwrap_or("").to_string(),
                mailbox: m.name.clone(),
                unread,
                key,
            });
        }
    }

    let total: u64 = folders.iter().map(|f| f.unread).sum();

    // Plugin NTFY : notifie seulement si le total augmente
    if prefs_snap.ntfy_enabled && !prefs_snap.ntfy_topic.is_empty() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static LAST_NTFY_TOTAL: AtomicU64 = AtomicU64::new(0);
        let prev = LAST_NTFY_TOTAL.load(Ordering::Relaxed);
        if total > prev {
            LAST_NTFY_TOTAL.store(total, Ordering::Relaxed);
            let server = prefs_snap.ntfy_server.clone();
            let topic = prefs_snap.ntfy_topic.clone();
            let title = format!("HimaWeb — {total} non-lu(s)");
            let body = folders
                .iter()
                .map(|f| format!("{}: {}", f.label, f.unread))
                .collect::<Vec<_>>()
                .join("\n");
            tokio::spawn(async move {
                if let Err(e) = crate::plugins::ntfy_publish(&server, &topic, &title, &body).await {
                    tracing::debug!("ntfy: {e}");
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

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
