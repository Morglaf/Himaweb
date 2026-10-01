use regex::Regex;
use std::sync::OnceLock;

const TRANSPARENT_PIXEL: &str =
    "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";

pub struct SanitizedBody {
    pub html: String,
    pub has_remote_content: bool,
    /// URLs http(s) bloquées (pour les lister dans les accessoires).
    pub remote_urls: Vec<String>,
}

pub fn sanitize_html(raw: &str) -> SanitizedBody {
    let cleaned = ammonia::Builder::default()
        .link_rel(Some("noopener noreferrer"))
        .add_tag_attributes("img", &["class", "data-remote-src"])
        // cid: requis pour les images inline MIME (signatures, icônes 20px, …)
        .add_url_schemes(["cid", "data"])
        .clean(raw)
        .to_string();
    block_remote_images(&cleaned)
}

/// Remplace `cid:…` par un placeholder ; l’URL locale va dans `data-remote-src`
/// (affichage seulement après validation, comme le contenu http distant).
pub fn rewrite_cid_images(
    html: &str,
    cid_map: &[(String, String)],
    mailbox: &str,
    message_id: &str,
    account: Option<&str>,
) -> (String, bool) {
    if cid_map.is_empty() {
        return (html.to_string(), false);
    }
    static CID_DQ: OnceLock<Regex> = OnceLock::new();
    static CID_SQ: OnceLock<Regex> = OnceLock::new();
    let cid_dq = CID_DQ
        .get_or_init(|| Regex::new(r#"(?i)\bsrc\s*=\s*"cid:([^"]+)""#).expect("cid dq"));
    let cid_sq = CID_SQ
        .get_or_init(|| Regex::new(r#"(?i)\bsrc\s*=\s*'cid:([^']+)'"#).expect("cid sq"));

    let lookup = |raw: &str| -> Option<String> {
        let key = normalize_cid(raw);
        cid_map
            .iter()
            .find(|(c, _)| c == &key)
            .map(|(_, id)| id.clone())
    };

    let to_url = |att_id: &str| -> String {
        let mut u = format!(
            "/attachments?mailbox={}&message_id={}&attachment_id={}&disposition=inline",
            urlencoding::encode(mailbox),
            urlencoding::encode(message_id),
            urlencoding::encode(att_id),
        );
        if let Some(a) = account.filter(|s| !s.is_empty()) {
            u.push_str("&account=");
            u.push_str(&urlencoding::encode(a));
        }
        u
    };

    let mut found = false;
    let html = cid_dq.replace_all(html, |caps: &regex::Captures| {
        let raw = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        if let Some(id) = lookup(raw) {
            found = true;
            let url = to_url(&id).replace('"', "&quot;");
            format!(
                r#"src="{TRANSPARENT_PIXEL}" class="remote-img" data-remote-src="{url}""#
            )
        } else {
            caps[0].to_string()
        }
    });
    let html = cid_sq.replace_all(&html, |caps: &regex::Captures| {
        let raw = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        if let Some(id) = lookup(raw) {
            found = true;
            let url = to_url(&id).replace('\'', "&#39;");
            format!(
                "src='{TRANSPARENT_PIXEL}' class=\"remote-img\" data-remote-src='{url}'"
            )
        } else {
            caps[0].to_string()
        }
    });
    (html.into_owned(), found)
}

fn normalize_cid(raw: &str) -> String {
    let decoded = urlencoding::decode(raw)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| raw.to_string());
    decoded
        .trim()
        .trim_matches(|c| c == '<' || c == '>')
        .trim()
        .to_ascii_lowercase()
}

pub fn plain_to_html(text: &str) -> String {
    let escaped = html_escape(text);
    format!(
        "<pre class=\"msg-plain\">{}</pre>",
        escaped.replace('\n', "<br>")
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn block_remote_images(html: &str) -> SanitizedBody {
    static IMG_RE: OnceLock<Regex> = OnceLock::new();
    static SRC_DQ: OnceLock<Regex> = OnceLock::new();
    static SRC_SQ: OnceLock<Regex> = OnceLock::new();
    static CLASS_DQ: OnceLock<Regex> = OnceLock::new();
    static CLASS_SQ: OnceLock<Regex> = OnceLock::new();
    static SRCSET_RE: OnceLock<Regex> = OnceLock::new();

    let img_re = IMG_RE.get_or_init(|| Regex::new(r#"(?is)<img\b([^>]*?)/?>"#).expect("img"));
    let src_dq = SRC_DQ.get_or_init(|| Regex::new(r#"(?i)\bsrc\s*=\s*"([^"]*)""#).expect("src dq"));
    let src_sq = SRC_SQ.get_or_init(|| Regex::new(r#"(?i)\bsrc\s*=\s*'([^']*)'"#).expect("src sq"));
    let class_dq =
        CLASS_DQ.get_or_init(|| Regex::new(r#"(?i)\bclass\s*=\s*"([^"]*)""#).expect("class dq"));
    let class_sq =
        CLASS_SQ.get_or_init(|| Regex::new(r#"(?i)\bclass\s*=\s*'([^']*)'"#).expect("class sq"));
    let srcset_re = SRCSET_RE.get_or_init(|| {
        Regex::new(r#"(?i)\bsrcset\s*=\s*("[^"]*"|'[^']*')"#).expect("srcset")
    });

    let html = srcset_re.replace_all(html, "").into_owned();

    let mut has_remote = false;
    let mut remote_urls: Vec<String> = Vec::new();
    let html = img_re
        .replace_all(&html, |caps: &regex::Captures| {
            let attrs = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let (src, quote, without_src) = if let Some(c) = src_dq.captures(attrs) {
                let src = c.get(1).map(|m| m.as_str()).unwrap_or("").trim().to_string();
                (src, "\"", src_dq.replace(attrs, "").to_string())
            } else if let Some(c) = src_sq.captures(attrs) {
                let src = c.get(1).map(|m| m.as_str()).unwrap_or("").trim().to_string();
                (src, "'", src_sq.replace(attrs, "").to_string())
            } else {
                return caps[0].to_string();
            };

            let lower = src.to_ascii_lowercase();
            // http(s) et URLs protocol-relative (`//cdn…`)
            let is_remote = lower.starts_with("http://")
                || lower.starts_with("https://")
                || lower.starts_with("//");
            if !is_remote {
                return caps[0].to_string();
            }
            has_remote = true;
            let abs = if lower.starts_with("//") {
                format!("https:{src}")
            } else {
                src.clone()
            };
            if !remote_urls.iter().any(|u| u == &abs) {
                remote_urls.push(abs.clone());
            }

            let attrs_out = if let Some(c) = class_dq.captures(&without_src) {
                let existing = c.get(1).map(|m| m.as_str()).unwrap_or("");
                let new_cls = merge_remote_class(existing);
                class_dq
                    .replace(&without_src, format!(r#"class="{new_cls}""#))
                    .trim()
                    .to_string()
            } else if let Some(c) = class_sq.captures(&without_src) {
                let existing = c.get(1).map(|m| m.as_str()).unwrap_or("");
                let new_cls = merge_remote_class(existing);
                class_sq
                    .replace(&without_src, format!("class='{new_cls}'"))
                    .trim()
                    .to_string()
            } else {
                let trimmed = without_src.trim();
                if trimmed.is_empty() {
                    "class=\"remote-img\"".into()
                } else {
                    format!("{trimmed} class=\"remote-img\"")
                }
            };

            let escaped_src = abs.replace('"', "&quot;");
            format!(
                "<img {attrs_out} src={quote}{TRANSPARENT_PIXEL}{quote} data-remote-src={quote}{escaped_src}{quote}>"
            )
        })
        .into_owned();

    SanitizedBody {
        html,
        has_remote_content: has_remote,
        remote_urls,
    }
}

pub fn remote_url_label(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    let name = path.rsplit('/').next().unwrap_or("").trim();
    if name.is_empty() || (name.contains('.') && name.len() > 64) {
        let host = url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or("distant");
        return format!("image · {host}");
    }
    if name.len() > 48 {
        format!("{}…", &name[..45])
    } else {
        name.to_string()
    }
}

fn merge_remote_class(existing: &str) -> String {
    if existing.split_whitespace().any(|t| t == "remote-img") {
        existing.to_string()
    } else if existing.is_empty() {
        "remote-img".into()
    } else {
        format!("{existing} remote-img")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_https_img() {
        let out = sanitize_html(r#"<p>Hi <img src="https://evil.example/t.png" alt="x"></p>"#);
        assert!(out.has_remote_content, "html={}", out.html);
        assert!(out.html.contains("data-remote-src="), "html={}", out.html);
        assert!(out.html.contains("remote-img"), "html={}", out.html);
        assert!(
            out.html.contains(TRANSPARENT_PIXEL),
            "html={}",
            out.html
        );
        assert!(
            out.html.contains("data-remote-src=\"https://evil.example/t.png\"")
                || out.html.contains("data-remote-src='https://evil.example/t.png'"),
            "html={}",
            out.html
        );
    }

    #[test]
    fn keeps_data_img() {
        let out = sanitize_html(r#"<img src="data:image/gif;base64,AAA" alt="x">"#);
        assert!(!out.has_remote_content, "html={}", out.html);
        assert!(!out.html.contains("data-remote-src="), "html={}", out.html);
    }

    #[test]
    fn keeps_cid_img() {
        let out = sanitize_html(r#"<img src="cid:part1@mail" width="20" height="20">"#);
        assert!(!out.has_remote_content, "html={}", out.html);
        assert!(
            out.html.contains("cid:part1@mail") || out.html.contains("cid:part1@mail"),
            "html={}",
            out.html
        );
        assert!(out.html.contains("src="), "html={}", out.html);
    }

    #[test]
    fn rewrites_cid_to_blocked_local_url() {
        let html = r#"<img src="cid:ico@letsignit" width="20" height="20">"#;
        let out = sanitize_html(html);
        let (rewritten, had) = rewrite_cid_images(
            &out.html,
            &[("ico@letsignit".into(), "3".into())],
            "INBOX",
            "42",
            Some("work"),
        );
        assert!(had);
        assert!(
            rewritten.contains("data-remote-src=")
                && rewritten.contains("/attachments?")
                && rewritten.contains("attachment_id=3")
                && rewritten.contains("remote-img"),
            "html={rewritten}"
        );
        assert!(!rewritten.contains("cid:"), "html={rewritten}");
        // Pas de chargement tant que data-remote-src n'est pas promu en src
        assert!(
            rewritten.contains(TRANSPARENT_PIXEL),
            "html={rewritten}"
        );
    }

    #[test]
    fn blocks_protocol_relative() {
        let out = sanitize_html(r#"<img src="//cdn.example/x.png" alt="x">"#);
        assert!(out.has_remote_content, "html={}", out.html);
        assert!(out.html.contains("data-remote-src="), "html={}", out.html);
        assert!(
            out.html.contains("https://cdn.example/x.png"),
            "html={}",
            out.html
        );
    }
}
