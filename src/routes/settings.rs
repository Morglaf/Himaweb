use std::sync::Arc;

use askama::Template;
use axum::extract::State;
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
        .route("/settings/thunderbird/import", post(import_thunderbird))
        .route("/settings/calendar/import", post(import_calendar))
        .route("/settings/calendar/add", post(add_caldav))
        .route("/settings/calendar/password", post(set_calendula_password))
        .route("/settings/calendar/delete", post(delete_cal_account))
        .route("/settings/contacts/import", post(import_contacts))
        .route("/settings/contacts/add", post(add_carddav))
        .route("/settings/contacts/password", post(set_cardamum_password))
        .route("/settings/contacts/delete", post(delete_card_account))
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
    pub editable: Vec<AccountEdit>,
    pub cardamum_accounts: Vec<CardamumAccountEdit>,
    pub calendula_accounts: Vec<CalendulaAccountEdit>,
    pub account_order: Vec<String>,
    pub color_accounts: Vec<ColorAccountRow>,
    pub move_defaults: Vec<MoveDefaultRow>,
    pub selected_account: String,
    pub all_selected: bool,
    pub folder_rows: Vec<FolderPrefRow>,
    pub thunderbird_profiles: Vec<String>,
    pub import_preview: Option<String>,
    pub import_message: Option<String>,
    pub calendar_preview: Option<String>,
    pub calendar_message: Option<String>,
    pub contacts_preview: Option<String>,
    pub contacts_message: Option<String>,
    pub notifications: bool,
    pub merged_inbox: bool,
    pub himalaya_available: bool,
    pub calendula_available: bool,
    pub cardamum_available: bool,
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
    pub icon_choices: Vec<IconChoice>,
}

pub struct IconChoice {
    pub id: String,
    pub selected: bool,
}

pub struct FolderPrefRow {
    pub key: String,
    pub label: String,
    pub pinned: bool,
    pub hidden: bool,
    pub watched: bool,
}

struct Flash {
    import_preview: Option<String>,
    import_message: Option<String>,
    calendar_preview: Option<String>,
    calendar_message: Option<String>,
    contacts_preview: Option<String>,
    contacts_message: Option<String>,
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
        .map(|n| {
            let icon = prefs_snap.account_icon(n);
            ColorAccountRow {
                color: prefs_snap.account_color(n),
                label: prefs_snap.account_label(n),
                icon: icon.clone(),
                icon_choices: crate::account_colors::ACCOUNT_ICON_CHOICES
                    .iter()
                    .map(|id| IconChoice {
                        selected: *id == icon,
                        id: (*id).into(),
                    })
                    .collect(),
                name: n.clone(),
            }
        })
        .collect();

    let editable = accounts_config::list_editable_accounts().unwrap_or_default();
    let cardamum_accounts = contacts_import::list_cardamum_accounts().unwrap_or_default();
    let calendula_accounts = calendar_import::list_calendula_accounts().unwrap_or_default();

    let folder_rows = {
        let _permit = state.cli_limit.acquire().await.ok();
        let scope = prefs_snap.selected_account();
        let boxes = if state.himalaya_available {
            state
                .himalaya
                .list_mailboxes(scope)
                .await
                .unwrap_or_default()
        } else {
            vec![]
        };
        boxes
            .into_iter()
            .map(|m| {
                let key = Prefs::folder_key(scope, &m.name);
                FolderPrefRow {
                    pinned: prefs_snap.is_pinned(&key),
                    hidden: prefs_snap.is_hidden(&key),
                    watched: prefs_snap.is_watched(&key, &m.name),
                    label: m.name.clone(),
                    key,
                }
            })
            .collect::<Vec<_>>()
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
        folder_rows,
        thunderbird_profiles,
        import_preview: flash.import_preview,
        import_message: flash.import_message,
        calendar_preview: flash.calendar_preview,
        calendar_message: flash.calendar_message,
        contacts_preview: flash.contacts_preview,
        contacts_message: flash.contacts_message,
        notifications: prefs_snap.notifications,
        merged_inbox: prefs_snap.merged_inbox,
        himalaya_available: state.himalaya_available,
        calendula_available: state.calendula_available,
        cardamum_available: state.cardamum_available,
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
    Form(form): Form<UiForm>,
) -> impl IntoResponse {
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

#[derive(Deserialize)]
pub struct MoveDefaultsForm {
    pub account: Vec<String>,
    pub folder: Vec<String>,
}

async fn save_move_defaults(
    State(state): State<Arc<AppState>>,
    Form(form): Form<MoveDefaultsForm>,
) -> impl IntoResponse {
    {
        let mut prefs = state.prefs.lock().await;
        prefs.default_move.clear();
        for (a, f) in form.account.iter().zip(form.folder.iter()) {
            let a = a.trim();
            let f = f.trim();
            if !a.is_empty() && !f.is_empty() {
                prefs.default_move.insert(a.to_string(), f.to_string());
            }
        }
        let _ = prefs.save();
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

#[derive(Deserialize)]
pub struct ColorsForm {
    #[serde(default)]
    pub name: Vec<String>,
    #[serde(default)]
    pub color: Vec<String>,
    #[serde(default)]
    pub label: Vec<String>,
    #[serde(default)]
    pub icon: Vec<String>,
}

async fn save_account_colors(
    State(state): State<Arc<AppState>>,
    Form(form): Form<ColorsForm>,
) -> impl IntoResponse {
    {
        let mut prefs = state.prefs.lock().await;
        let n = form.name.len();
        for i in 0..n {
            let name = form.name.get(i).map(|s| s.trim()).unwrap_or("");
            if name.is_empty() {
                continue;
            }
            if let Some(c) = form.color.get(i).map(|s| s.trim()) {
                if c.starts_with('#') && (c.len() == 7 || c.len() == 4) {
                    prefs
                        .account_colors
                        .insert(name.to_string(), c.to_ascii_lowercase());
                }
            }
            if let Some(label) = form.label.get(i) {
                let l = label.trim();
                if l.is_empty() || l == name {
                    prefs.account_labels.remove(name);
                } else {
                    prefs.account_labels.insert(name.to_string(), l.to_string());
                }
            }
            if let Some(icon) = form.icon.get(i) {
                let ic = icon.trim();
                if ic.is_empty() || ic == "circle-user" {
                    prefs.account_icons.remove(name);
                } else if crate::account_colors::ACCOUNT_ICON_CHOICES.contains(&ic) {
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
}

async fn edit_account(
    State(state): State<Arc<AppState>>,
    Form(form): Form<EditAccountForm>,
) -> impl IntoResponse {
    let make_default =
        form.make_default.as_deref() == Some("1") || form.make_default.as_deref() == Some("on");
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
    ) {
        Ok(()) => {
            let mut flash = Flash::empty();
            flash.import_message = Some(format!("Compte « {} » mis à jour (mots de passe inclus si renseignés).", form.name));
            render_settings(state, flash).await
        }
        Err(e) => {
            let mut flash = Flash::empty();
            flash.import_message = Some(format!("Édition échouée: {e}"));
            render_settings(state, flash).await
        }
    }
}

#[derive(Deserialize)]
pub struct FoldersForm {
    #[serde(default)]
    pub pinned: Vec<String>,
    #[serde(default)]
    pub hidden: Vec<String>,
    #[serde(default)]
    pub watched: Vec<String>,
}

async fn save_folders(
    State(state): State<Arc<AppState>>,
    Form(form): Form<FoldersForm>,
) -> impl IntoResponse {
    {
        let mut prefs = state.prefs.lock().await;
        prefs.pinned_folders = form.pinned;
        prefs.hidden_folders = form.hidden;
        prefs.watched_folders = form.watched;
        *prefs = prefs.clone().normalize();
        let _ = prefs.save();
    }
    Redirect::to("/settings#folders").into_response()
}

#[derive(Deserialize)]
pub struct NotifyForm {
    pub notifications: Option<String>,
    pub merged_inbox: Option<String>,
}

async fn save_notify(
    State(state): State<Arc<AppState>>,
    Form(form): Form<NotifyForm>,
) -> impl IntoResponse {
    {
        let mut prefs = state.prefs.lock().await;
        prefs.notifications =
            form.notifications.as_deref() == Some("1") || form.notifications.as_deref() == Some("on");
        prefs.merged_inbox =
            form.merged_inbox.as_deref() == Some("1") || form.merged_inbox.as_deref() == Some("on");
        let _ = prefs.save();
    }
    Redirect::to("/settings#folders").into_response()
}

#[derive(Deserialize)]
pub struct ImportForm {
    pub profile: String,
    pub write: Option<String>,
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
            let toml = thunderbird::to_himalaya_toml(&accounts);
            let write = form.write.as_deref() == Some("1") || form.write.as_deref() == Some("on");
            let message = if write {
                let dest = prefs::himalaya_config_path();
                if let Some(parent) = dest.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if dest.exists() {
                    Some(format!(
                        "Le fichier {} existe déjà — aperçu sans écrasement. Utilisez « Modifier un compte » pour les mots de passe.",
                        dest.display()
                    ))
                } else {
                    match std::fs::write(&dest, &toml) {
                        Ok(()) => Some(format!(
                            "{} compte(s) écrits. Ajoutez les mots de passe dans « Modifier un compte » (pas besoin d’éditer le TOML).",
                            accounts.len()
                        )),
                        Err(e) => Some(format!("Écriture impossible: {e}")),
                    }
                }
            } else {
                Some(format!(
                    "{} compte(s) détecté(s). Vérifiez l'aperçu, puis cochez « Écrire config.toml ».",
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

