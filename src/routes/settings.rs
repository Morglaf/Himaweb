use std::sync::Arc;

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;

use crate::accounts_config::{self, AccountEdit};
use crate::calendar_import::{self, CalendulaAccountEdit};
use crate::contacts_import::{self, CardamumAccountEdit};
use crate::prefs::{self, Prefs, ACCOUNT_ALL};
use crate::state::AppState;
use crate::thunderbird;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/settings", get(settings_page))
        .route("/settings/ui", post(save_ui))
        .route("/settings/ui/sizes", post(save_ui_sizes))
        .route("/settings/move-defaults", post(save_move_defaults))
        .route("/settings/account", post(save_account))
        .route("/settings/account/order", post(save_account_order))
        .route("/settings/account/colors", post(save_account_colors))
        .route("/settings/account/edit", post(edit_account))
        .route("/settings/account/delete", post(delete_mail_account))
        .route("/settings/folders", post(save_folders))
        .route("/settings/notify", post(save_notify))
        .route("/settings/conversations", post(toggle_conversations))
        .route("/settings/thunderbird/import", post(import_thunderbird))
        .route("/settings/thunderbird/accounts", get(thunderbird_accounts))
        .route("/settings/calendar/import", post(import_calendar))
        .route("/settings/calendar/add", post(add_caldav))
        .route("/settings/calendar/password", post(set_calendula_password))
        .route("/settings/calendar/delete", post(delete_cal_account))
        .route("/settings/calendar/colors", post(save_cal_colors))
        .route("/settings/contacts/import", post(import_contacts))
        .route("/settings/contacts/add", post(add_carddav))
        .route("/settings/contacts/password", post(set_cardamum_password))
        .route("/settings/contacts/delete", post(delete_card_account))
        .route("/settings/ortie/auth", post(ortie_auth))
        .route("/settings/neverest/sync", post(neverest_sync))
        .route("/settings/mirador/watch", post(mirador_watch))
        .route("/settings/plugins/install", post(plugins_install))
        .route("/settings/plugins/remove", post(plugins_remove))
        .route("/settings/ntfy", post(save_ntfy))
        .route("/settings/ai", post(save_ai))
        .route("/settings/ai/models", post(list_ai_models))
}

#[derive(Template)]
#[template(path = "shell.html")]
struct ShellTemplate {
    pub title: String,
    pub active_tab: String,
    pub offline: bool,
    pub himalaya_available: bool,
    pub calendula_available: bool,
    pub cardamum_available: bool,
    pub theme: String,
    pub layout: String,
    pub ui_style: String,
    pub error: Option<String>,
    pub content: String,
}

#[derive(Template)]
#[template(path = "settings.html")]
struct SettingsTemplate {
    pub theme: String,
    pub layout: String,
    pub ui_font_scale: f32,
    pub ui_radius: u16,
    pub ui_space: f32,
    pub ui_rail: u16,
    pub ui_list: u16,
    pub config_path: String,
    pub config_exists: bool,
    pub calendula_path: String,
    pub calendula_exists: bool,
    pub cardamum_path: String,
    pub cardamum_exists: bool,
    pub accounts: Vec<AccountRow>,
    pub editable: Vec<EditableAccountRow>,
    pub cardamum_accounts: Vec<CardamumAccountEdit>,
    pub calendula_accounts: Vec<CalendulaAccountEdit>,
    pub account_order: Vec<String>,
    pub color_accounts: Vec<ColorAccountRow>,
    pub move_defaults: Vec<MoveDefaultRow>,
    pub selected_account: String,
    pub all_selected: bool,
    pub folder_groups: Vec<FolderPrefGroup>,
    pub thunderbird_profiles: Vec<String>,
    pub import_preview: Option<String>,
    pub import_message: Option<String>,
    pub calendar_preview: Option<String>,
    pub calendar_message: Option<String>,
    pub contacts_preview: Option<String>,
    pub contacts_message: Option<String>,
    pub notifications: bool,
    pub merged_inbox: bool,
    pub conversations: bool,
    pub side_widget: bool,
    pub side_widget_events: u16,
    pub himalaya_available: bool,
    pub calendula_available: bool,
    pub cardamum_available: bool,
    pub ortie_available: bool,
    pub neverest_available: bool,
    pub mirador_available: bool,
    pub mirador_enabled: bool,
    pub ortie_message: Option<String>,
    pub neverest_message: Option<String>,
    pub mirador_message: Option<String>,
    pub plugins_dir: String,
    pub plugins: Vec<PluginRow>,
    pub plugins_message: Option<String>,
    pub ntfy_enabled: bool,
    pub ntfy_server: String,
    pub ntfy_topic: String,
    pub ai_enabled: bool,
    pub ai_provider: String,
    pub ai_endpoint: String,
    pub ai_model: String,
    pub ai_remote_endpoint: String,
    pub ai_api_key: String,
    pub ai_api_key_set: bool,
    pub ai_gemini_model: String,
    pub ai_message: Option<String>,
    pub cal_color_accounts: Vec<ColorAccountRow>,
}

pub struct PluginRow {
    pub id: String,
    pub name: String,
    pub description: String,
}

pub struct MoveDefaultRow {
    pub account: String,
    pub options: Vec<MoveFolderOpt>,
}

pub struct MoveFolderOpt {
    pub name: String,
    pub selected: bool,
}

pub struct AccountRow {
    pub name: String,
    pub backends: String,
    pub is_default: bool,
}

pub struct ColorAccountRow {
    pub name: String,
    pub label: String,
    pub icon: String,
    pub color: String,
}

pub struct FolderPrefRow {
    pub key: String,
    pub label: String,
    pub pinned: bool,
    pub hidden: bool,
    pub watched: bool,
}

pub struct FolderPrefGroup {
    pub account: String,
    pub folders: Vec<FolderPrefRow>,
}

/// Compte éditable + dossiers IMAP pour les menus d’alias (poubelle, etc.).
pub struct EditableAccountRow {
    pub name: String,
    pub email: String,
    pub display_name: String,
    pub imap_server: String,
    pub imap_user: String,
    pub smtp_server: String,
    pub smtp_user: String,
    pub is_default: bool,
    pub has_imap_password: bool,
    pub has_smtp_password: bool,
    pub trash_alias: String,
    pub sent_alias: String,
    pub drafts_alias: String,
    pub mailboxes: Vec<MailboxAliasChoice>,
    /// Pref HimaWeb : pas de UID MOVE (COPY + purge)
    pub copy_move: bool,
}

pub struct MailboxAliasChoice {
    pub name: String,
    pub is_trash: bool,
    pub is_sent: bool,
    pub is_drafts: bool,
}

impl EditableAccountRow {
    fn from_edit(a: AccountEdit, mailbox_names: Vec<String>, copy_move: bool) -> Self {
        let mut names = mailbox_names;
        for alias in [&a.trash_alias, &a.sent_alias, &a.drafts_alias] {
            let alias = alias.trim();
            if !alias.is_empty()
                && !names.iter().any(|m| m.eq_ignore_ascii_case(alias))
            {
                names.insert(0, alias.to_string());
            }
        }
        let mailboxes = names
            .into_iter()
            .map(|name| MailboxAliasChoice {
                is_trash: name.eq_ignore_ascii_case(&a.trash_alias),
                is_sent: name.eq_ignore_ascii_case(&a.sent_alias),
                is_drafts: name.eq_ignore_ascii_case(&a.drafts_alias),
                name,
            })
            .collect();
        Self {
            name: a.name,
            email: a.email,
            display_name: a.display_name,
            imap_server: a.imap_server,
            imap_user: a.imap_user,
            smtp_server: a.smtp_server,
            smtp_user: a.smtp_user,
            is_default: a.is_default,
            has_imap_password: a.has_imap_password,
            has_smtp_password: a.has_smtp_password,
            trash_alias: a.trash_alias,
            sent_alias: a.sent_alias,
            drafts_alias: a.drafts_alias,
            mailboxes,
            copy_move,
        }
    }
}

struct Flash {
    import_preview: Option<String>,
    import_message: Option<String>,
    calendar_preview: Option<String>,
    calendar_message: Option<String>,
    contacts_preview: Option<String>,
    contacts_message: Option<String>,
    ortie_message: Option<String>,
    neverest_message: Option<String>,
    mirador_message: Option<String>,
    plugins_message: Option<String>,
    ai_message: Option<String>,
}

impl Flash {
    fn empty() -> Self {
        Self {
            import_preview: None,
            import_message: None,
            calendar_preview: None,
            calendar_message: None,
            contacts_preview: None,
            contacts_message: None,
            ortie_message: None,
            neverest_message: None,
            mirador_message: None,
            plugins_message: None,
            ai_message: None,
        }
    }
}

async fn settings_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    render_settings(state, Flash::empty()).await
}

async fn render_settings(state: Arc<AppState>, flash: Flash) -> axum::response::Response {
    let prefs_snap = state.prefs.lock().await.clone();
    let theme = prefs_snap.theme.clone();
    let layout = prefs_snap.layout.clone();
    let selected = prefs_snap.account.clone().unwrap_or_default();
    let all_selected = prefs_snap.is_all_accounts();

    let config_path = prefs::himalaya_config_path().display().to_string();
    let config_exists = prefs::himalaya_config_exists();
    let calendula_path = prefs::calendula_config_path().display().to_string();
    let calendula_exists = prefs::calendula_config_exists();
    let cardamum_path = prefs::cardamum_config_path().display().to_string();
    let cardamum_exists = prefs::cardamum_config_exists();

    let accounts = if state.himalaya_available && config_exists {
        let _permit = state.cli_limit.acquire().await.ok();
        match state.himalaya.list_accounts().await {
            Ok(list) => list
                .into_iter()
                .map(|a| AccountRow {
                    name: a.name,
                    backends: a.backends,
                    is_default: a.is_default,
                })
                .collect(),
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    let known: Vec<String> = accounts.iter().map(|a| a.name.clone()).collect();
    let account_order = prefs_snap.ordered_accounts(&known);
    let color_accounts: Vec<ColorAccountRow> = account_order
        .iter()
        .map(|n| ColorAccountRow {
            color: prefs_snap.account_color(n),
            label: prefs_snap.account_label(n),
            icon: prefs_snap.account_icon(n),
            name: n.clone(),
        })
        .collect();

    let editable_raw = accounts_config::list_editable_accounts().unwrap_or_default();
    let cardamum_accounts = contacts_import::list_cardamum_accounts().unwrap_or_default();
    let calendula_accounts = calendar_import::list_calendula_accounts().unwrap_or_default();

    let editable = {
        let _permit = state.cli_limit.acquire().await.ok();
        let mut rows = Vec::with_capacity(editable_raw.len());
        for a in editable_raw {
            let copy_move = prefs_snap.uses_copy_move(&a.name);
            let boxes = if state.himalaya_available {
                state
                    .himalaya
                    .list_mailboxes(Some(&a.name))
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|m| m.name)
                    .collect::<Vec<_>>()
            } else {
                vec![]
            };
            rows.push(EditableAccountRow::from_edit(a, boxes, copy_move));
        }
        rows
    };
    let folder_groups = {
        let _permit = state.cli_limit.acquire().await.ok();
        let mut groups = Vec::new();
        if state.himalaya_available {
            for acc in &account_order {
                let boxes = state
                    .himalaya
                    .list_mailboxes(Some(acc))
                    .await
                    .unwrap_or_default();
                let folders = boxes
                    .into_iter()
                    .map(|m| {
                        let key = Prefs::folder_key(Some(acc), &m.name);
                        FolderPrefRow {
                            pinned: prefs_snap.is_pinned(&key),
                            hidden: prefs_snap.is_hidden(&key),
                            watched: prefs_snap.is_watched(&key, &m.name),
                            label: m.name.clone(),
                            key,
                        }
                    })
                    .collect::<Vec<_>>();
                if !folders.is_empty() {
                    groups.push(FolderPrefGroup {
                        account: acc.clone(),
                        folders,
                    });
                }
            }
        }
        groups
    };

    let mut move_defaults: Vec<MoveDefaultRow> = Vec::new();
    if state.himalaya_available {
        let _permit = state.cli_limit.acquire().await.ok();
        for acc in &account_order {
            let current = prefs_snap
                .default_move_for(acc)
                .unwrap_or("")
                .to_string();
            let options = state
                .himalaya
                .list_mailboxes(Some(acc))
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|m| MoveFolderOpt {
                    selected: m.name.eq_ignore_ascii_case(&current),
                    name: m.name,
                })
                .collect::<Vec<_>>();
            move_defaults.push(MoveDefaultRow {
                account: acc.clone(),
                options,
            });
        }
    }

    let thunderbird_profiles = thunderbird::discover_profiles()
        .into_iter()
        .map(|p| p.display().to_string())
        .collect();

    let cal_color_accounts: Vec<ColorAccountRow> = calendula_accounts
        .iter()
        .map(|a| ColorAccountRow {
            color: prefs_snap.cal_account_color(&a.name),
            label: prefs_snap.cal_account_label(&a.name),
            icon: prefs_snap.cal_account_icon(&a.name),
            name: a.name.clone(),
        })
        .collect();

    let ai_gemini_model = if prefs_snap.ai_provider == "gemini" {
        prefs_snap.ai_model.clone()
    } else {
        "gemini-2.5-flash".into()
    };
    let ai_model_display = if prefs_snap.ai_provider == "gemini" {
        ai_gemini_model.clone()
    } else {
        prefs_snap.ai_model.clone()
    };

    let inner = SettingsTemplate {
        theme: theme.clone(),
        layout: layout.clone(),
        ui_font_scale: prefs_snap.ui_font_scale,
        ui_radius: prefs_snap.ui_radius,
        ui_space: prefs_snap.ui_space,
        ui_rail: prefs_snap.ui_rail,
        ui_list: prefs_snap.ui_list,
        config_path,
        config_exists,
        calendula_path,
        calendula_exists,
        cardamum_path,
        cardamum_exists,
        accounts,
        editable,
        cardamum_accounts,
        calendula_accounts,
        account_order,
        color_accounts,
        move_defaults,
        selected_account: selected,
        all_selected,
        folder_groups,
        thunderbird_profiles,
        import_preview: flash.import_preview,
        import_message: flash.import_message,
        calendar_preview: flash.calendar_preview,
        calendar_message: flash.calendar_message,
        contacts_preview: flash.contacts_preview,
        contacts_message: flash.contacts_message,
        notifications: prefs_snap.notifications,
        merged_inbox: prefs_snap.merged_inbox,
        conversations: prefs_snap.conversations,
        side_widget: prefs_snap.side_widget,
        side_widget_events: prefs_snap.side_widget_events.max(1),
        himalaya_available: state.himalaya_available,
        calendula_available: state.calendula_available,
        cardamum_available: state.cardamum_available,
        ortie_available: state.ortie_available,
        neverest_available: state.neverest_available,
        mirador_available: state.mirador_available,
        mirador_enabled: prefs_snap.mirador_enabled,
        ortie_message: flash.ortie_message,
        neverest_message: flash.neverest_message,
        mirador_message: flash.mirador_message,
        plugins_dir: crate::plugins::plugins_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "(indisponible)".into()),
        plugins: crate::plugins::list_plugins()
            .into_iter()
            .map(|p| PluginRow {
                id: p.id,
                name: p.name,
                description: p.description,
            })
            .collect(),
        plugins_message: flash.plugins_message,
        ntfy_enabled: prefs_snap.ntfy_enabled,
        ntfy_server: prefs_snap.ntfy_server.clone(),
        ntfy_topic: prefs_snap.ntfy_topic.clone(),
        ai_enabled: prefs_snap.ai_enabled,
        ai_provider: prefs_snap.ai_provider.clone(),
        ai_endpoint: prefs_snap.ai_endpoint.clone(),
        ai_model: ai_model_display,
        ai_remote_endpoint: prefs_snap.ai_remote_endpoint.clone(),
        ai_api_key: String::new(),
        ai_api_key_set: !prefs_snap.ai_api_key.is_empty(),
        ai_gemini_model,
        ai_message: flash.ai_message,
        cal_color_accounts,
    };

    let content = match inner.render() {
        Ok(c) => c,
        Err(e) => format!("<pre>{e}</pre>"),
    };

    let shell = ShellTemplate {
        title: "HimaWeb — Paramètres".into(),
        active_tab: "settings".into(),
        offline: false,
        himalaya_available: state.himalaya_available,
        calendula_available: state.calendula_available,
        cardamum_available: state.cardamum_available,
        theme,
        layout,
        ui_style: prefs_snap.ui_style_attr(),
        error: None,
        content,
    };
    match shell.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => Html(format!("<pre>{e}</pre>")).into_response(),
    }
}

#[derive(Deserialize)]
pub struct UiForm {
    pub theme: String,
    pub layout: String,
    pub ui_font_scale: Option<f32>,
    pub ui_radius: Option<u16>,
    pub ui_space: Option<f32>,
    pub ui_rail: Option<u16>,
    pub ui_list: Option<u16>,
}

async fn save_ui(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Form(form): Form<UiForm>,
) -> impl IntoResponse {
    let quiet = headers.get("HX-Request").is_some();
    {
        let mut prefs = state.prefs.lock().await;
        prefs.theme = form.theme;
        prefs.layout = form.layout;
        if let Some(v) = form.ui_font_scale {
            prefs.ui_font_scale = v;
        }
        if let Some(v) = form.ui_radius {
            prefs.ui_radius = v;
        }
        if let Some(v) = form.ui_space {
            prefs.ui_space = v;
        }
        if let Some(v) = form.ui_rail {
            prefs.ui_rail = v;
        }
        if let Some(v) = form.ui_list {
            prefs.ui_list = v;
        }
        *prefs = prefs.clone().normalize();
        let _ = prefs.save();
    }
    if quiet {
        return axum::http::StatusCode::NO_CONTENT.into_response();
    }
    Redirect::to("/settings").into_response()
}

#[derive(Deserialize)]
pub struct UiSizesForm {
    pub rail: Option<u16>,
    pub list: Option<u16>,
}

async fn save_ui_sizes(
    State(state): State<Arc<AppState>>,
    Form(form): Form<UiSizesForm>,
) -> impl IntoResponse {
    {
        let mut prefs = state.prefs.lock().await;
        if let Some(v) = form.rail {
            prefs.ui_rail = v;
        }
        if let Some(v) = form.list {
            prefs.ui_list = v;
        }
        *prefs = prefs.clone().normalize();
        let _ = prefs.save();
    }
    axum::http::StatusCode::NO_CONTENT
}

async fn save_move_defaults(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::extract::RawForm(raw): axum::extract::RawForm,
) -> impl IntoResponse {
    let quiet = headers.get("HX-Request").is_some();
    let map = crate::form_util::parse_form_lists(&raw);
    let accounts = crate::form_util::form_values(&map, "account");
    let folders = crate::form_util::form_values(&map, "folder");
    {
        let mut prefs = state.prefs.lock().await;
        prefs.default_move.clear();
        for (a, f) in accounts.iter().zip(folders.iter()) {
            let a = a.trim();
            let f = f.trim();
            if !a.is_empty() && !f.is_empty() {
                prefs.default_move.insert(a.to_string(), f.to_string());
            }
        }
        let _ = prefs.save();
    }
    if quiet {
        return axum::http::StatusCode::NO_CONTENT.into_response();
    }
    Redirect::to("/settings#folders").into_response()
}

#[derive(Deserialize)]
pub struct AccountForm {
    pub account: String,
}

async fn save_account(
    State(state): State<Arc<AppState>>,
    Form(form): Form<AccountForm>,
) -> impl IntoResponse {
    {
        let mut prefs = state.prefs.lock().await;
        let a = form.account.trim();
        prefs.account = if a.is_empty() {
            None
        } else {
            Some(a.to_string())
        };
        let _ = prefs.save();
    }
    Redirect::to("/settings").into_response()
}

#[derive(Deserialize)]
pub struct OrderForm {
    pub order: String,
}

async fn save_account_order(
    State(state): State<Arc<AppState>>,
    Form(form): Form<OrderForm>,
) -> impl IntoResponse {
    {
        let mut prefs = state.prefs.lock().await;
        prefs.account_order = form
            .order
            .split(|c| c == ',' || c == '\n' || c == ';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != ACCOUNT_ALL)
            .collect();
        *prefs = prefs.clone().normalize();
        let _ = prefs.save();
    }
    Redirect::to("/settings#accounts").into_response()
}

async fn save_account_colors(
    State(state): State<Arc<AppState>>,
    axum::extract::RawForm(raw): axum::extract::RawForm,
) -> impl IntoResponse {
    let map = crate::form_util::parse_form_lists(&raw);
    let names = crate::form_util::form_values(&map, "name");
    let colors = crate::form_util::form_values(&map, "color");
    let labels = crate::form_util::form_values(&map, "label");
    let icons = crate::form_util::form_values(&map, "icon");
    {
        let mut prefs = state.prefs.lock().await;
        let n = names.len();
        for i in 0..n {
            let name = names.get(i).map(|s| s.trim()).unwrap_or("");
            if name.is_empty() {
                continue;
            }
            if let Some(c) = colors.get(i).map(|s| s.trim()) {
                if c.starts_with('#') && (c.len() == 7 || c.len() == 4) {
                    prefs
                        .account_colors
                        .insert(name.to_string(), c.to_ascii_lowercase());
                }
            }
            if let Some(label) = labels.get(i) {
                let l = label.trim();
                if l.is_empty() || l == name {
                    prefs.account_labels.remove(name);
                } else {
                    prefs.account_labels.insert(name.to_string(), l.to_string());
                }
            }
            if let Some(icon) = icons.get(i) {
                let ic = icon.trim();
                if ic.is_empty() || ic == "circle-user" {
                    prefs.account_icons.remove(name);
                } else if crate::form_util::is_safe_icon_name(ic) {
                    prefs.account_icons.insert(name.to_string(), ic.to_string());
                }
            }
        }
        let _ = prefs.save();
    }
    Redirect::to("/settings#accounts").into_response()
}

#[derive(Deserialize)]
pub struct EditAccountForm {
    pub name: String,
    pub email: String,
    pub display_name: String,
    pub imap_server: String,
    pub imap_user: String,
    pub imap_password: Option<String>,
    pub smtp_server: String,
    pub smtp_user: String,
    pub smtp_password: Option<String>,
    pub make_default: Option<String>,
    pub trash_alias: Option<String>,
    pub sent_alias: Option<String>,
    pub drafts_alias: Option<String>,
    pub copy_move: Option<String>,
}

async fn edit_account(
    State(state): State<Arc<AppState>>,
    Form(form): Form<EditAccountForm>,
) -> impl IntoResponse {
    let make_default =
        form.make_default.as_deref() == Some("1") || form.make_default.as_deref() == Some("on");
    let copy_move =
        form.copy_move.as_deref() == Some("1") || form.copy_move.as_deref() == Some("on");
    let imap_pw = form
        .imap_password
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let smtp_pw = form
        .smtp_password
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    match accounts_config::update_account(
        &form.name,
        &form.email,
        &form.display_name,
        &form.imap_server,
        &form.imap_user,
        imap_pw,
        &form.smtp_server,
        &form.smtp_user,
        smtp_pw,
        make_default,
        form.trash_alias.as_deref(),
        form.sent_alias.as_deref(),
        form.drafts_alias.as_deref(),
    ) {
        Ok(()) => {
            {
                let mut prefs = state.prefs.lock().await;
                prefs.set_copy_move(&form.name, copy_move);
                let _ = prefs.save();
            }
            let mut flash = Flash::empty();
            flash.import_message = Some(format!(
                "Compte « {} » mis à jour{}.",
                form.name,
                if copy_move {
                    " — déplacements via COPY (sans UID MOVE)"
                } else {
                    ""
                }
            ));
            render_settings(state, flash).await
        }
        Err(e) => {
            let mut flash = Flash::empty();
            flash.import_message = Some(format!("Édition échouée: {e}"));
            render_settings(state, flash).await
        }
    }
}

fn merge_pref_keys(existing: &mut Vec<String>, account: &str, submitted: Vec<String>) {
    let prefix = format!("{account}::");
    existing.retain(|k| !k.starts_with(&prefix));
    for k in submitted {
        if k.starts_with(&prefix) && !existing.iter().any(|e| e == &k) {
            existing.push(k);
        }
    }
}

async fn save_folders(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    axum::extract::RawForm(raw): axum::extract::RawForm,
) -> impl IntoResponse {
    let quiet = headers.get("HX-Request").is_some();
    let map = crate::form_util::parse_form_lists(&raw);
    let Some(account) = crate::form_util::form_values(&map, "account")
        .first()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
    else {
        if quiet {
            return axum::http::StatusCode::BAD_REQUEST.into_response();
        }
        return Redirect::to("/settings#folders").into_response();
    };
    let pinned = crate::form_util::form_values(&map, "pinned")
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let hidden = crate::form_util::form_values(&map, "hidden")
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let watched = crate::form_util::form_values(&map, "watched")
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    {
        let mut prefs = state.prefs.lock().await;
        merge_pref_keys(&mut prefs.pinned_folders, &account, pinned);
        merge_pref_keys(&mut prefs.hidden_folders, &account, hidden);
        merge_pref_keys(&mut prefs.watched_folders, &account, watched);
        *prefs = prefs.clone().normalize();
        if let Err(e) = prefs.save() {
            tracing::warn!("prefs save folders: {e}");
            if quiet {
                return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    }
    if quiet {
        return axum::http::StatusCode::NO_CONTENT.into_response();
    }
    Redirect::to("/settings#folders").into_response()
}

#[derive(Deserialize)]
pub struct NotifyForm {
    pub notifications: Option<String>,
    pub merged_inbox: Option<String>,
    pub conversations: Option<String>,
    pub side_widget: Option<String>,
    pub side_widget_events: Option<u16>,
}

async fn save_notify(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Form(form): Form<NotifyForm>,
) -> impl IntoResponse {
    let quiet = headers.get("HX-Request").is_some();
    {
        let mut prefs = state.prefs.lock().await;
        prefs.notifications =
            form.notifications.as_deref() == Some("1") || form.notifications.as_deref() == Some("on");
        prefs.merged_inbox =
            form.merged_inbox.as_deref() == Some("1") || form.merged_inbox.as_deref() == Some("on");
        prefs.conversations = form.conversations.as_deref() == Some("1")
            || form.conversations.as_deref() == Some("on");
        prefs.side_widget =
            form.side_widget.as_deref() == Some("1") || form.side_widget.as_deref() == Some("on");
        if let Some(n) = form.side_widget_events {
            prefs.side_widget_events = n.clamp(1, 30);
        }
        let _ = prefs.save();
    }
    if quiet {
        return axum::http::StatusCode::NO_CONTENT.into_response();
    }
    Redirect::to("/settings#folders").into_response()
}

#[derive(Deserialize)]
pub struct ConversationsForm {
    pub enabled: Option<String>,
}

async fn toggle_conversations(
    State(state): State<Arc<AppState>>,
    Form(form): Form<ConversationsForm>,
) -> impl IntoResponse {
    {
        let mut prefs = state.prefs.lock().await;
        prefs.conversations =
            form.enabled.as_deref() == Some("1") || form.enabled.as_deref() == Some("on");
        let _ = prefs.save();
    }
    axum::http::StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
pub struct ImportForm {
    pub profile: String,
    pub write: Option<String>,
    #[serde(default, deserialize_with = "crate::form_util::deserialize_string_or_seq")]
    pub accounts: Vec<String>,
}

#[derive(Deserialize)]
pub struct TbAccountsQuery {
    pub profile: String,
}

async fn thunderbird_accounts(Query(q): Query<TbAccountsQuery>) -> impl IntoResponse {
    let path = std::path::PathBuf::from(&q.profile);
    match thunderbird::parse_prefs_js(&path) {
        Ok(accounts) => axum::Json(serde_json::json!({
            "accounts": accounts.into_iter().map(|a| serde_json::json!({
                "name": a.name,
                "email": a.email,
                "display_name": a.display_name,
            })).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

async fn import_thunderbird(
    State(state): State<Arc<AppState>>,
    Form(form): Form<ImportForm>,
) -> impl IntoResponse {
    let path = std::path::PathBuf::from(&form.profile);
    match thunderbird::parse_prefs_js(&path) {
        Ok(accounts) if accounts.is_empty() => {
            let mut flash = Flash::empty();
            flash.import_message =
                Some("Aucun compte IMAP/POP trouvé dans ce profil Thunderbird.".into());
            render_settings(state, flash).await
        }
        Ok(accounts) => {
            let selected: Vec<String> = form
                .accounts
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let accounts: Vec<_> = if selected.is_empty() {
                accounts
            } else {
                accounts
                    .into_iter()
                    .filter(|a| {
                        selected.iter().any(|s| {
                            s.eq_ignore_ascii_case(&a.name) || s.eq_ignore_ascii_case(&a.email)
                        })
                    })
                    .collect()
            };
            if accounts.is_empty() {
                let mut flash = Flash::empty();
                flash.import_message =
                    Some("Aucun compte sélectionné parmi ceux détectés.".into());
                return render_settings(state, flash).await;
            }
            let use_ortie = state.ortie_available;
            let toml = thunderbird::to_himalaya_toml(&accounts, use_ortie);
            let write = form.write.as_deref() == Some("1") || form.write.as_deref() == Some("on");
            let message = if write {
                match crate::accounts_config::merge_thunderbird_accounts(&accounts, use_ortie) {
                    Ok(msg) => Some(msg),
                    Err(e) => Some(format!("Écriture impossible: {e}")),
                }
            } else {
                let hint = if use_ortie {
                    "Les comptes Gmail seront ajoutés en OAuth (Ortie)."
                } else {
                    "Ortie introuvable : Gmail sera en mot de passe (PLAIN)."
                };
                Some(format!(
                    "{} compte(s) sélectionné(s). Vérifiez l'aperçu, puis cochez « Ajouter au config.toml ». {hint}",
                    accounts.len()
                ))
            };
            let mut flash = Flash::empty();
            flash.import_preview = Some(toml);
            flash.import_message = message;
            render_settings(state, flash).await
        }
        Err(e) => {
            let mut flash = Flash::empty();
            flash.import_message = Some(format!("Import échoué: {e}"));
            render_settings(state, flash).await
        }
    }
}

#[derive(Deserialize)]
pub struct CalendarImportForm {
    pub profile: String,
    pub write: Option<String>,
    pub from_mail: Option<String>,
}

async fn import_calendar(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CalendarImportForm>,
) -> impl IntoResponse {
    let path = std::path::PathBuf::from(&form.profile);
    let from_mail =
        form.from_mail.as_deref() == Some("1") || form.from_mail.as_deref() == Some("on");
    let write = form.write.as_deref() == Some("1") || form.write.as_deref() == Some("on");

    let (toml, msg_base) = if from_mail {
        match thunderbird::parse_prefs_js(&path) {
            Ok(accounts) => {
                let t = calendar_import::calendula_toml_from_mail(&accounts);
                (
                    t,
                    format!(
                        "Proposition CalDAV à partir de {} compte(s) mail.",
                        accounts.len()
                    ),
                )
            }
            Err(e) => {
                let mut flash = Flash::empty();
                flash.calendar_message = Some(format!("Import calendrier échoué: {e}"));
                return render_settings(state, flash).await;
            }
        }
    } else {
        match calendar_import::parse_calendars(&path) {
            Ok(cals) => {
                let n_caldav = cals
                    .iter()
                    .filter(|c| {
                        !c.disabled
                            && (c.cal_type.eq_ignore_ascii_case("caldav")
                                || c.uri.starts_with("http"))
                    })
                    .count();
                let t = calendar_import::calendula_toml_from_caldav(&cals);
                (
                    t,
                    format!(
                        "{} agenda(s) Thunderbird, dont {} CalDAV.",
                        cals.len(),
                        n_caldav
                    ),
                )
            }
            Err(e) => {
                let mut flash = Flash::empty();
                flash.calendar_message = Some(format!("Lecture calendriers échouée: {e}"));
                return render_settings(state, flash).await;
            }
        }
    };

    let message = if write {
        match calendar_import::write_calendula_config(&toml, true) {
            Ok(p) => Some(format!(
                "{msg_base} Écrit dans {}. Ajoutez les mots de passe via le formulaire ci-dessous.",
                p.display()
            )),
            Err(e) => Some(format!("{msg_base} Écriture: {e}")),
        }
    } else {
        Some(format!(
            "{msg_base} Vérifiez l'aperçu, puis cochez « Écrire config Calendula »."
        ))
    };

    let mut flash = Flash::empty();
    flash.calendar_preview = Some(toml);
    flash.calendar_message = message;
    render_settings(state, flash).await
}

#[derive(Deserialize)]
pub struct CaldavForm {
    pub name: String,
    pub server: String,
    pub username: String,
    pub password: String,
    pub make_default: Option<String>,
}

async fn add_caldav(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CaldavForm>,
) -> impl IntoResponse {
    let make_default =
        form.make_default.as_deref() == Some("1") || form.make_default.as_deref() == Some("on");
    let mut flash = Flash::empty();
    match calendar_import::upsert_caldav_account(
        &form.name,
        &form.server,
        &form.username,
        &form.password,
        make_default,
    ) {
        Ok(p) => {
            flash.calendar_message = Some(format!(
                "Compte CalDAV « {} » enregistré dans {}.",
                form.name,
                p.display()
            ));
        }
        Err(e) => {
            flash.calendar_message = Some(format!("Ajout CalDAV échoué: {e}"));
        }
    }
    render_settings(state, flash).await
}

#[derive(Deserialize)]
pub struct ContactsImportForm {
    pub profile: String,
    pub write_cardamum: Option<String>,
    pub import_local: Option<String>,
}

async fn import_contacts(
    State(state): State<Arc<AppState>>,
    Form(form): Form<ContactsImportForm>,
) -> impl IntoResponse {
    let path = std::path::PathBuf::from(&form.profile);
    let write = form.write_cardamum.as_deref() == Some("1")
        || form.write_cardamum.as_deref() == Some("on");
    let import_local =
        form.import_local.as_deref() == Some("1") || form.import_local.as_deref() == Some("on");

    let mut flash = Flash::empty();
    let mut msgs = Vec::new();

    match contacts_import::parse_address_books(&path) {
        Ok(books) => {
            let n_dav = books.iter().filter(|b| !b.carddav_url.is_empty()).count();
            let toml = contacts_import::cardamum_toml_from_books(&books);
            msgs.push(format!(
                "{} carnet(s) Thunderbird, dont {} CardDAV.",
                books.len(),
                n_dav
            ));
            if write {
                match contacts_import::write_cardamum_config(&toml, true) {
                    Ok(p) => msgs.push(format!(
                        "Config Cardamum écrite dans {}. Renseignez les mots de passe ci-dessous.",
                        p.display()
                    )),
                    Err(e) => msgs.push(format!("Écriture Cardamum: {e}")),
                }
            } else {
                msgs.push(
                    "Aperçu généré — cochez « Écrire config Cardamum » pour enregistrer.".into(),
                );
            }
            flash.contacts_preview = Some(toml);
        }
        Err(e) => msgs.push(format!("Lecture carnets: {e}")),
    }

    if import_local {
        match contacts_import::import_local_contacts(&path) {
            Ok(contacts) => {
                let pairs: Vec<(String, String)> = contacts
                    .iter()
                    .map(|c| (c.email.clone(), c.name.clone()))
                    .collect();
                let n = pairs.len();
                let cache = state.cache.lock().await;
                match cache.save_contacts(&pairs) {
                    Ok(()) => msgs.push(format!(
                        "{n} contact(s) locaux importés dans le cache HimaWeb (autocomplete)."
                    )),
                    Err(e) => msgs.push(format!("Cache contacts: {e}")),
                }
            }
            Err(e) => msgs.push(format!("Import abook.sqlite: {e}")),
        }
    }

    flash.contacts_message = Some(msgs.join(" "));
    render_settings(state, flash).await
}

#[derive(Deserialize)]
pub struct CarddavForm {
    pub name: String,
    pub uri: String,
    pub username: String,
    pub password: String,
    pub make_default: Option<String>,
}

async fn add_carddav(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CarddavForm>,
) -> impl IntoResponse {
    let make_default =
        form.make_default.as_deref() == Some("1") || form.make_default.as_deref() == Some("on");
    let mut flash = Flash::empty();
    match contacts_import::upsert_carddav_account(
        &form.name,
        &form.uri,
        &form.username,
        &form.password,
        make_default,
    ) {
        Ok(p) => {
            flash.contacts_message = Some(format!(
                "Compte CardDAV « {} » enregistré dans {}.",
                form.name,
                p.display()
            ));
        }
        Err(e) => {
            flash.contacts_message = Some(format!("Ajout CardDAV échoué: {e}"));
        }
    }
    render_settings(state, flash).await
}

#[derive(Deserialize)]
pub struct PasswordForm {
    pub name: String,
    pub password: String,
}

async fn set_cardamum_password(
    State(state): State<Arc<AppState>>,
    Form(form): Form<PasswordForm>,
) -> impl IntoResponse {
    let mut flash = Flash::empty();
    if form.password.trim().is_empty() {
        flash.contacts_message = Some("Mot de passe vide — rien changé.".into());
    } else {
        match contacts_import::set_carddav_password(&form.name, form.password.trim()) {
            Ok(()) => {
                flash.contacts_message =
                    Some(format!("Mot de passe Cardamum mis à jour pour « {} ».", form.name));
            }
            Err(e) => flash.contacts_message = Some(format!("Échec: {e}")),
        }
    }
    render_settings(state, flash).await
}

async fn set_calendula_password(
    State(state): State<Arc<AppState>>,
    Form(form): Form<PasswordForm>,
) -> impl IntoResponse {
    let mut flash = Flash::empty();
    if form.password.trim().is_empty() {
        flash.calendar_message = Some("Mot de passe vide — rien changé.".into());
    } else {
        match calendar_import::set_caldav_password(&form.name, form.password.trim()) {
            Ok(()) => {
                flash.calendar_message =
                    Some(format!("Mot de passe Calendula mis à jour pour « {} ».", form.name));
            }
            Err(e) => flash.calendar_message = Some(format!("Échec: {e}")),
        }
    }
    render_settings(state, flash).await
}

#[derive(Deserialize)]
pub struct DeleteForm {
    pub name: String,
}

async fn delete_mail_account(
    State(state): State<Arc<AppState>>,
    Form(form): Form<DeleteForm>,
) -> impl IntoResponse {
    let mut flash = Flash::empty();
    match crate::config_fix::delete_himalaya_account(form.name.trim()) {
        Ok(()) => {
            flash.import_message = Some(format!("Compte mail « {} » supprimé.", form.name));
        }
        Err(e) => flash.import_message = Some(format!("Suppression: {e}")),
    }
    render_settings(state, flash).await
}

async fn delete_cal_account(
    State(state): State<Arc<AppState>>,
    Form(form): Form<DeleteForm>,
) -> impl IntoResponse {
    let mut flash = Flash::empty();
    match crate::config_fix::delete_calendula_account(form.name.trim()) {
        Ok(()) => {
            flash.calendar_message = Some(format!("Compte calendrier « {} » supprimé.", form.name));
        }
        Err(e) => flash.calendar_message = Some(format!("Suppression: {e}")),
    }
    render_settings(state, flash).await
}

async fn delete_card_account(
    State(state): State<Arc<AppState>>,
    Form(form): Form<DeleteForm>,
) -> impl IntoResponse {
    let mut flash = Flash::empty();
    match crate::config_fix::delete_cardamum_account(form.name.trim()) {
        Ok(()) => {
            flash.contacts_message = Some(format!("Compte contacts « {} » supprimé.", form.name));
        }
        Err(e) => flash.contacts_message = Some(format!("Suppression: {e}")),
    }
    render_settings(state, flash).await
}

#[allow(dead_code)]
pub async fn shell_chrome(state: &AppState) -> (String, String) {
    state.theme_layout().await
}

#[derive(Deserialize)]
pub struct OrtieForm {
    pub account: Option<String>,
}

async fn ortie_auth(
    State(state): State<Arc<AppState>>,
    Form(form): Form<OrtieForm>,
) -> impl IntoResponse {
    let mut flash = Flash::empty();
    let Some(client) = &state.ortie else {
        flash.ortie_message = Some("Ortie introuvable — installez le binaire ou définissez HIMAWEB_ORTIE_BIN.".into());
        return render_settings(state, flash).await;
    };
    let _permit = state.cli_limit.acquire().await.ok();
    match client.authorize(form.account.as_deref()).await {
        Ok(out) => {
            flash.ortie_message = Some(if out.is_empty() {
                "OAuth Ortie lancé / terminé.".into()
            } else {
                out
            });
        }
        Err(e) => flash.ortie_message = Some(format!("Ortie: {e}")),
    }
    drop(_permit);
    render_settings(state, flash).await
}

#[derive(Deserialize)]
pub struct NeverestForm {
    pub account: Option<String>,
}

async fn neverest_sync(
    State(state): State<Arc<AppState>>,
    Form(form): Form<NeverestForm>,
) -> impl IntoResponse {
    let mut flash = Flash::empty();
    let Some(client) = &state.neverest else {
        flash.neverest_message = Some("Neverest introuvable — installez le binaire ou HIMAWEB_NEVEREST_BIN.".into());
        return render_settings(state, flash).await;
    };
    let _permit = state.cli_limit.acquire().await.ok();
    match client.sync(form.account.as_deref()).await {
        Ok(out) => {
            flash.neverest_message = Some(if out.is_empty() {
                "Sync Neverest terminée.".into()
            } else {
                out.chars().take(800).collect()
            });
        }
        Err(e) => flash.neverest_message = Some(format!("Neverest: {e}")),
    }
    drop(_permit);
    render_settings(state, flash).await
}

#[derive(Deserialize)]
pub struct MiradorForm {
    pub enabled: Option<String>,
}

async fn mirador_watch(
    State(state): State<Arc<AppState>>,
    Form(form): Form<MiradorForm>,
) -> impl IntoResponse {
    let mut flash = Flash::empty();
    let enabled = form.enabled.as_deref() == Some("1");
    {
        let mut p = state.prefs.lock().await;
        p.mirador_enabled = enabled;
        let _ = p.save();
    }
    if enabled {
        if let Some(client) = &state.mirador {
            let _permit = state.cli_limit.acquire().await.ok();
            match client.status().await {
                Ok(s) => {
                    flash.mirador_message = Some(format!(
                        "Watch activé. Mirador: {}",
                        s.chars().take(200).collect::<String>()
                    ));
                }
                Err(e) => {
                    flash.mirador_message =
                        Some(format!("Watch enregistré, mais statut Mirador: {e}"));
                }
            }
            drop(_permit);
        } else {
            flash.mirador_message =
                Some("Watch coché mais Mirador absent — poll navigateur conservé.".into());
        }
    } else {
        flash.mirador_message = Some("Watch Mirador désactivé — poll navigateur seul.".into());
    }
    render_settings(state, flash).await
}

#[derive(Deserialize)]
pub struct PluginInstallForm {
    pub repo: String,
}

async fn plugins_install(
    State(state): State<Arc<AppState>>,
    Form(form): Form<PluginInstallForm>,
) -> impl IntoResponse {
    let mut flash = Flash::empty();
    match crate::plugins::install_from_git(&form.repo) {
        Ok(p) => {
            flash.plugins_message = Some(format!("Plugin « {} » installé.", p.name));
        }
        Err(e) => flash.plugins_message = Some(format!("Installation: {e}")),
    }
    render_settings(state, flash).await
}

#[derive(Deserialize)]
pub struct PluginRemoveForm {
    pub id: String,
}

async fn plugins_remove(
    State(state): State<Arc<AppState>>,
    Form(form): Form<PluginRemoveForm>,
) -> impl IntoResponse {
    let mut flash = Flash::empty();
    match crate::plugins::remove_plugin(&form.id) {
        Ok(()) => flash.plugins_message = Some(format!("Plugin « {} » retiré.", form.id)),
        Err(e) => flash.plugins_message = Some(format!("Retrait: {e}")),
    }
    render_settings(state, flash).await
}

#[derive(Deserialize)]
pub struct NtfyForm {
    pub enabled: Option<String>,
    pub server: String,
    pub topic: String,
}

async fn save_ntfy(
    State(state): State<Arc<AppState>>,
    Form(form): Form<NtfyForm>,
) -> impl IntoResponse {
    let mut flash = Flash::empty();
    {
        let mut p = state.prefs.lock().await;
        p.ntfy_enabled = form.enabled.as_deref() == Some("1");
        p.ntfy_server = form.server.trim().to_string();
        if p.ntfy_server.is_empty() {
            p.ntfy_server = "https://ntfy.sh".into();
        }
        p.ntfy_topic = form.topic.trim().to_string();
        let _ = p.save();
    }
    flash.plugins_message = Some("Préférences NTFY enregistrées.".into());
    render_settings(state, flash).await
}

#[derive(Deserialize)]
pub struct AiSettingsForm {
    pub enabled: Option<String>,
    pub provider: Option<String>,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub gemini_model: Option<String>,
    pub remote_endpoint: Option<String>,
    pub api_key: Option<String>,
}

async fn save_ai(
    State(state): State<Arc<AppState>>,
    Form(form): Form<AiSettingsForm>,
) -> impl IntoResponse {
    let mut flash = Flash::empty();
    {
        let mut p = state.prefs.lock().await;
        p.ai_enabled = form.enabled.as_deref() == Some("1");
        let provider = form
            .provider
            .as_deref()
            .unwrap_or("ollama")
            .trim()
            .to_ascii_lowercase();
        p.ai_provider = if provider == "gemini" {
            "gemini".into()
        } else {
            "ollama".into()
        };
        if p.ai_provider == "gemini" {
            let gm = form
                .gemini_model
                .as_deref()
                .unwrap_or("gemini-2.5-flash")
                .trim()
                .trim_start_matches("models/");
            p.ai_model = if gm.is_empty() {
                "gemini-2.5-flash".into()
            } else {
                gm.to_string()
            };
            if let Some(key) = form.api_key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                p.ai_api_key = key.to_string();
            }
        } else {
            p.ai_endpoint = form
                .endpoint
                .as_deref()
                .unwrap_or("http://127.0.0.1:11434")
                .trim()
                .to_string();
            if p.ai_endpoint.is_empty() {
                p.ai_endpoint = "http://127.0.0.1:11434".into();
            }
            p.ai_model = form
                .model
                .as_deref()
                .unwrap_or("llama3.2")
                .trim()
                .to_string();
            if p.ai_model.is_empty() {
                p.ai_model = "llama3.2".into();
            }
            p.ai_remote_endpoint = form
                .remote_endpoint
                .unwrap_or_default()
                .trim()
                .to_string();
        }
        let _ = p.save();
    }
    flash.ai_message = Some("Préférences IA enregistrées.".into());
    render_settings(state, flash).await
}

#[derive(Deserialize)]
pub struct AiModelsForm {
    pub api_key: Option<String>,
}

async fn list_ai_models(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<AiModelsForm>,
) -> impl IntoResponse {
    use axum::Json;
    use serde_json::json;

    let prefs = state.prefs.lock().await.clone();
    let key = body
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(prefs.ai_api_key.trim());
    if key.is_empty() {
        return Json(json!({ "error": "Clé API Gemini manquante" })).into_response();
    }
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models?key={key}"
    );
    let client = reqwest::Client::new();
    match client.get(&url).send().await {
        Ok(res) if res.status().is_success() => {
            let v: serde_json::Value = match res.json().await {
                Ok(v) => v,
                Err(e) => return Json(json!({ "error": e.to_string() })).into_response(),
            };
            let mut models: Vec<String> = v
                .get("models")
                .and_then(|m| m.as_array())
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    let name = m.get("name")?.as_str()?;
                    let methods = m.get("supportedGenerationMethods")?.as_array()?;
                    let ok = methods.iter().any(|x| x.as_str() == Some("generateContent"));
                    if !ok {
                        return None;
                    }
                    Some(name.trim_start_matches("models/").to_string())
                })
                .collect();
            models.sort();
            models.dedup();
            Json(json!({ "models": models })).into_response()
        }
        Ok(res) => {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            Json(json!({ "error": format!("HTTP {status}: {body}") })).into_response()
        }
        Err(e) => Json(json!({ "error": e.to_string() })).into_response(),
    }
}

#[allow(dead_code)]
struct _CalColorsUnused;

async fn save_cal_colors(
    State(state): State<Arc<AppState>>,
    axum::extract::RawForm(raw): axum::extract::RawForm,
) -> impl IntoResponse {
    let mut flash = Flash::empty();
    let map = crate::form_util::parse_form_lists(&raw);
    let names = crate::form_util::form_values(&map, "name");
    let colors = crate::form_util::form_values(&map, "color");
    let labels = crate::form_util::form_values(&map, "label");
    let icons = crate::form_util::form_values(&map, "icon");
    {
        let mut p = state.prefs.lock().await;
        let n = names.len();
        for i in 0..n {
            let name = names.get(i).map(|s| s.trim()).unwrap_or("");
            if name.is_empty() {
                continue;
            }
            if let Some(c) = colors.get(i).map(|s| s.trim()) {
                if c.starts_with('#') && (c.len() == 7 || c.len() == 4) {
                    p.cal_account_colors
                        .insert(name.to_string(), c.to_ascii_lowercase());
                }
            }
            if let Some(label) = labels.get(i) {
                let l = label.trim();
                if l.is_empty() || l == name {
                    p.cal_account_labels.remove(name);
                } else {
                    p.cal_account_labels.insert(name.to_string(), l.to_string());
                }
            }
            if let Some(icon) = icons.get(i) {
                let ic = icon.trim();
                if ic.is_empty() || ic == "calendar" {
                    p.cal_account_icons.remove(name);
                } else if crate::form_util::is_safe_icon_name(ic) {
                    p.cal_account_icons.insert(name.to_string(), ic.to_string());
                }
            }
        }
        let _ = p.save();
    }
    flash.calendar_message = Some("Apparence des agendas enregistrée.".into());
    render_settings(state, flash).await
}


