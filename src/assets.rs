//! Assets statiques embarqués dans le binaire.
//!
//! Les bibliothèques front (HTMX, Alpine, Lucide, Quill) sont servies depuis le
//! binaire plutôt que depuis un CDN : HimaWeb tourne en 127.0.0.1 et ne doit pas
//! attendre le réseau — ni échouer hors-ligne — pour afficher sa première page.
//!
//! Chaque asset porte un ETag dérivé de son contenu, et `version()` fournit
//! l'empreinte globale injectée dans les URLs (`?v=…`). Une modification de
//! `static/` change donc l'URL automatiquement, sans version à bumper à la main.

use std::sync::OnceLock;

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

const JS: &str = "application/javascript; charset=utf-8";
const CSS: &str = "text/css; charset=utf-8";
const JSON: &str = "application/json; charset=utf-8";

/// URLs versionnées : le navigateur peut garder l'asset indéfiniment, un
/// changement de contenu produit une nouvelle URL.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

struct Asset {
    route: &'static str,
    body: &'static str,
    mime: &'static str,
}

const ASSETS: &[Asset] = &[
    Asset {
        route: "/static/app.css",
        body: include_str!("../static/app.css"),
        mime: CSS,
    },
    Asset {
        route: "/static/app.js",
        body: include_str!("../static/app.js"),
        mime: JS,
    },
    Asset {
        route: "/static/vendor/htmx.min.js",
        body: include_str!("../static/vendor/htmx.min.js"),
        mime: JS,
    },
    Asset {
        route: "/static/vendor/alpine.min.js",
        body: include_str!("../static/vendor/alpine.min.js"),
        mime: JS,
    },
    Asset {
        route: "/static/vendor/lucide.min.js",
        body: include_str!("../static/vendor/lucide.min.js"),
        mime: JS,
    },
    Asset {
        route: "/static/vendor/quill.js",
        body: include_str!("../static/vendor/quill.js"),
        mime: JS,
    },
    Asset {
        route: "/static/vendor/quill.snow.css",
        body: include_str!("../static/vendor/quill.snow.css"),
        mime: CSS,
    },
    Asset {
        route: "/static/vendor/lucide-tags.json",
        body: include_str!("../static/vendor/lucide-tags.json"),
        mime: JSON,
    },
];

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn etags() -> &'static [String] {
    static ETAGS: OnceLock<Vec<String>> = OnceLock::new();
    ETAGS.get_or_init(|| {
        ASSETS
            .iter()
            .map(|a| format!("\"{:016x}\"", fnv1a(a.body.as_bytes())))
            .collect()
    })
}

/// Empreinte de l'ensemble des assets, à injecter dans les URLs statiques.
pub fn version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION
        .get_or_init(|| {
            let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
            for a in ASSETS {
                hash ^= fnv1a(a.body.as_bytes());
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
            format!("{hash:016x}")
        })
        .as_str()
}

fn serve(idx: usize, headers: &HeaderMap) -> Response {
    let asset = &ASSETS[idx];
    let etag = &etags()[idx];

    let cached = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|candidate| candidate.trim() == etag));

    let mut resp = if cached {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        (StatusCode::OK, asset.body).into_response()
    };

    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(asset.mime));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(IMMUTABLE));
    if let Ok(value) = HeaderValue::from_str(etag) {
        h.insert(header::ETAG, value);
    }
    resp
}

pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let mut router = Router::new();
    for (idx, asset) in ASSETS.iter().enumerate() {
        router = router.route(
            asset.route,
            get(move |headers: HeaderMap| async move { serve(idx, &headers) }),
        );
    }
    router
}
