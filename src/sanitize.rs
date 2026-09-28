pub fn sanitize_html(raw: &str) -> String {
    ammonia::Builder::default()
        .link_rel(Some("noopener noreferrer"))
        .clean(raw)
        .to_string()
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
