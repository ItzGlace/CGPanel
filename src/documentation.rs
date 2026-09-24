use super::*;
use pulldown_cmark::{html, Event, Options, Parser, Tag, TagEnd};

pub fn routes() -> Router<App> {
    Router::new()
        .route("/docs", get(index))
        .route("/docs/{slug}", get(article))
}
const DOCS: &[(&str, &str, &str, bool)] = &[
    (
        "getting-started",
        "Getting started",
        include_str!("../docs/GETTING-STARTED.md"),
        false,
    ),
    (
        "features",
        "Monitoring, backups and HTTPS",
        include_str!("../docs/V0.2-GUIDE.md"),
        false,
    ),
    (
        "operations",
        "Administrator operations",
        include_str!("../docs/OPERATIONS.md"),
        true,
    ),
    (
        "api",
        "Administrator API guide",
        include_str!("../docs/API.md"),
        true,
    ),
    (
        "reference",
        "Endpoint reference",
        include_str!("../docs/API-REFERENCE.md"),
        true,
    ),
];
async fn index(Extension(user): Extension<Identity>) -> Json<Value> {
    Json(json!(DOCS
        .iter()
        .filter(|d| !d.3 || user.role == "admin")
        .map(|d| json!({"slug":d.0,"title":d.1}))
        .collect::<Vec<_>>()))
}
async fn article(Extension(user): Extension<Identity>, Path(slug): Path<String>) -> Api<Value> {
    let doc = DOCS
        .iter()
        .find(|d| d.0 == slug)
        .ok_or_else(|| Error(StatusCode::NOT_FOUND, "Document not found".into()))?;
    if doc.3 {
        admin_api::require_admin(&user)?;
    }
    // Only bundled Markdown is rendered. Escape raw HTML and prohibit active URL schemes.
    let mut suppressed_link = false;
    let parser = Parser::new_ext(
        doc.2,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH,
    )
    .filter_map(|event| match event {
        Event::Html(s) | Event::InlineHtml(s) => Some(Event::Text(s)),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let allowed = dest_url.starts_with("https://") || dest_url.starts_with('#');
            suppressed_link = !allowed;
            if allowed {
                Some(Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                }))
            } else {
                None
            }
        }
        Event::End(TagEnd::Link) if suppressed_link => {
            suppressed_link = false;
            None
        }
        event => Some(event),
    });
    let mut rendered = String::new();
    html::push_html(&mut rendered, parser);
    Ok(Json(
        json!({"slug":doc.0,"title":doc.1,"html":rendered,"markdown":doc.2}),
    ))
}
