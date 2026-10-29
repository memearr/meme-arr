//! Serving the single-page app, with the configured URL base injected.
//!
//! The built `index.html` carries a `window.__MEMEARR_BASE__=""` placeholder
//! (see `web/index.html`); the frontend prefixes every request and link with
//! it, and its assets are relative (`base: "./"` in the Vite config), so the
//! same build works at the root or under a sub-path like `/meme-arr/`.

use crate::sync::RwLockExt;
use axum::{
    extract::{OriginalUri, Request, State},
    http::Uri,
    middleware::Next,
    response::{Html, IntoResponse, Redirect, Response},
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use tracing::warn;

const PLACEHOLDER: &str = r#"window.__MEMEARR_BASE__="""#;

/// `html` with the URL base filled in. `base` has already been validated by
/// `config::normalize_url_base` (URL-safe characters only), so it's safe to
/// place inside a JS string.
pub fn inject_base(html: &str, base: &str) -> String {
    html.replace(PLACEHOLDER, &format!(r#"window.__MEMEARR_BASE__="{base}""#))
}

/// Read `index.html` from the built SPA, once, with the base injected. A
/// missing file (API-only dev setups, tests) yields an empty page and a
/// warning rather than failing startup.
pub fn load_index(web_dir: &Path, base: &str) -> String {
    match std::fs::read_to_string(web_dir.join("index.html")) {
        Ok(html) => {
            if !base.is_empty() && !html.contains(PLACEHOLDER) {
                warn!(
                    "URL_BASE is set but index.html has no base placeholder -- \
                     the frontend won't know to use it (was the web build skipped?)"
                );
            }
            inject_base(&html, base)
        }
        Err(e) => {
            warn!("couldn't read {}/index.html: {e}", web_dir.display());
            String::new()
        }
    }
}

/// The served `index.html`, re-read whenever the file changes on disk. It used
/// to be read once at startup, so rebuilding the frontend under a running
/// server left it pointing at asset filenames that no longer existed (a blank
/// page until restart). One `stat` per page load is the cost.
pub struct IndexPage {
    web_dir: PathBuf,
    base: String,
    cache: RwLock<(Option<SystemTime>, Arc<String>)>,
}

impl IndexPage {
    pub fn new(web_dir: &Path, base: &str) -> Self {
        let html = Arc::new(load_index(web_dir, base));
        Self {
            web_dir: web_dir.to_path_buf(),
            base: base.to_string(),
            cache: RwLock::new((Self::mtime(web_dir), html)),
        }
    }

    fn mtime(web_dir: &Path) -> Option<SystemTime> {
        std::fs::metadata(web_dir.join("index.html"))
            .and_then(|m| m.modified())
            .ok()
    }

    /// The current page, reloaded first if the file changed (or appeared, or
    /// vanished) since it was last read.
    pub fn get(&self) -> Arc<String> {
        let now = Self::mtime(&self.web_dir);
        {
            let c = self.cache.read_locked();
            if c.0 == now {
                return c.1.clone();
            }
        }
        let html = Arc::new(load_index(&self.web_dir, &self.base));
        *self.cache.write_locked() = (now, html.clone());
        html
    }
}

/// The page for a request whose original (pre-prefix-stripping) URI is
/// `uri`. The bare base (`/meme-arr`, no trailing slash) redirects to
/// `/meme-arr/`, since the page's relative asset URLs only resolve against
/// the slash form; anything else gets the page.
pub fn index_response(html: &str, uri: &Uri, base: &str) -> Response {
    if !base.is_empty() && uri.path() == base {
        Redirect::permanent(&format!("{base}/")).into_response()
    } else {
        Html(html.to_owned()).into_response()
    }
}

/// Middleware: serve the in-memory index for `/` and `/index.html` (as seen
/// by the app, i.e. after any URL-base prefix is stripped). ServeDir can't do
/// this itself -- it never calls its fallback for a directory request, and
/// its own `index.html` wouldn't have the base injected.
pub async fn serve_index(
    State((page, base)): State<(Arc<IndexPage>, Arc<String>)>,
    OriginalUri(original): OriginalUri,
    req: Request,
    next: Next,
) -> Response {
    match req.uri().path() {
        "" | "/" | "/index.html" => index_response(&page.get(), &original, &base),
        _ => next.run(req).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML: &str =
        r#"<script>window.__MEMEARR_BASE__="";</script><script src="./assets/a.js"></script>"#;

    #[test]
    fn index_page_follows_the_file_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("index.html");
        std::fs::write(&file, "<p>v1</p>").unwrap();
        let page = IndexPage::new(dir.path(), "");
        let first = page.get();
        assert_eq!(*first, "<p>v1</p>");
        // Unchanged file: the very same cached page, no re-read.
        assert!(Arc::ptr_eq(&first, &page.get()));

        // A rebuild replaces the file (new mtime): the next request sees it.
        std::fs::write(&file, "<p>v2</p>").unwrap();
        let f = std::fs::OpenOptions::new().write(true).open(&file).unwrap();
        f.set_modified(SystemTime::now() + std::time::Duration::from_secs(60))
            .unwrap();
        assert_eq!(*page.get(), "<p>v2</p>");
    }

    #[test]
    fn index_page_gets_the_base_injected_on_every_reload() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("index.html");
        std::fs::write(&file, HTML).unwrap();
        let page = IndexPage::new(dir.path(), "/meme-arr");
        assert!(page.get().contains(r#"__MEMEARR_BASE__="/meme-arr""#));
        std::fs::write(&file, HTML.replace("a.js", "b.js")).unwrap();
        let f = std::fs::OpenOptions::new().write(true).open(&file).unwrap();
        f.set_modified(SystemTime::now() + std::time::Duration::from_secs(60))
            .unwrap();
        let out = page.get();
        assert!(out.contains("b.js") && out.contains(r#"__MEMEARR_BASE__="/meme-arr""#));
    }

    #[test]
    fn the_base_replaces_only_the_placeholder() {
        let out = inject_base(HTML, "/meme-arr");
        assert!(out.contains(r#"window.__MEMEARR_BASE__="/meme-arr";"#));
        assert!(out.contains(r#"src="./assets/a.js""#));
    }

    #[test]
    fn an_empty_base_leaves_the_page_unchanged() {
        assert_eq!(inject_base(HTML, ""), HTML);
    }

    #[test]
    fn the_bare_base_redirects_to_its_trailing_slash_form_and_pages_are_served_otherwise() {
        let page =
            |p: &str, base: &str| index_response("<p/>", &p.parse::<Uri>().unwrap(), base).status();
        assert_eq!(page("/", ""), 200);
        assert_eq!(page("/meme-arr/", "/meme-arr"), 200);
        assert_eq!(page("/meme-arr/index.html", "/meme-arr"), 200);
        assert_eq!(page("/meme-arr/some/unknown/path", "/meme-arr"), 200);
        assert_eq!(page("/meme-arr", "/meme-arr"), 308);
        // With no base, a path is never treated as "the bare base".
        assert_eq!(page("/anything", ""), 200);
    }

    #[test]
    fn a_page_without_the_placeholder_is_returned_as_is() {
        assert_eq!(inject_base("<p>hi</p>", "/x"), "<p>hi</p>");
    }
}
