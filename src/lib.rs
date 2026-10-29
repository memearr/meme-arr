//! meme-arr library: clip & meme studio for Plex and Jellyfin.
//!
//! Exposes the modules so integration tests can exercise the pipeline, and
//! provides [`run`] as the server entry point used by the binary.

pub mod api;
pub mod auth;
pub mod backup;
pub mod byterange;
pub mod compact_rebuild;
pub mod compactindex;
pub mod config;
pub mod db;
pub mod diagnose;
pub mod diskstats;
pub mod export;
pub mod ffmpeg;
pub mod health;
pub mod index;
pub mod jellyfin;
pub mod logs;
pub mod mediaserver;
pub mod oidc;
pub mod paths;
pub mod plex;
pub mod plex_auth;
pub mod preview_cache;
pub mod proxies;
pub mod querysyntax;
pub mod ratelimit;
pub mod remote_sub;
pub mod spa;
pub mod state;
pub mod subtitles;
pub mod sync;
pub mod tasks;
pub mod version;

use anyhow::{Context, Result};
use config::Config;
use state::{AppState, IndexStatus};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

/// Build state, start the initial index, and serve the HTTP API + SPA.
pub async fn run(logs: Arc<logs::LogControl>) -> Result<()> {
    let started_at = std::time::Instant::now();
    let config = Config::from_env().context("invalid configuration")?;
    info!("starting meme-arr; bind={}", config.bind_addr);

    std::fs::create_dir_all(&config.data_dir).ok();
    std::fs::create_dir_all(&config.output_dir).ok();
    std::fs::create_dir_all(config.tmp_dir()).ok();

    let pool = db::open_pool(&config.db_path()).context("opening database")?;

    // Seed settings from env on first run (migration path for existing
    // env-configured deployments), then load the live settings from the DB.
    seed_settings_from_env(&pool, &config)?;
    let settings = {
        let conn = pool.get()?;
        state::AppSettings::load(&conn)?
    };

    // Recovery/bootstrap: RESET_ADMIN_PASSWORD, if set, (re)writes the
    // stored admin password every time the app starts. This is deliberately
    // loud and deliberately not auto-cleared -- it's a one-time operator
    // action (set it, restart, log in, remove it), and clearing it
    // ourselves would be surprising for something security-sensitive.
    if let Some(new_password) = &config.reset_admin_password {
        let conn = pool.get()?;
        let hash = auth::hash_password(new_password).context("hashing RESET_ADMIN_PASSWORD")?;
        db::set_admin_password_hash(&conn, &hash)?;
        warn!(
            "RESET_ADMIN_PASSWORD was set -- admin password has been reset. \
             Remove this env var and restart once you've logged in with the new password."
        );
    }

    let state = AppState {
        config: Arc::new(config.clone()),
        pool,
        settings: Arc::new(RwLock::new(settings)),
        index: Arc::new(Mutex::new(IndexStatus::default())),
        jobs: Arc::new(Mutex::new(HashMap::new())),
        compact: Arc::new(RwLock::new(None)),
        started_at,
        login_limiter: Arc::new(ratelimit::LoginLimiter::default()),
        running_tasks: Arc::new(Mutex::new(std::collections::HashSet::new())),
        health: Arc::new(RwLock::new(health::Report::default())),
        logs,
    };

    // A level chosen in the UI beats RUST_LOG, and survives restarts.
    if let Some(level) = db::get_setting(&*state.pool.get()?, "log_level")? {
        if let Err(e) = state.logs.set_level(&level) {
            warn!("ignoring saved log level: {e}");
        }
    }

    {
        let s = state.clone();
        tokio::spawn(async move {
            compact_rebuild::load_or_rebuild(s).await;
        });
    }

    // Library scans (and other maintenance) run on their Tasks schedule. A
    // restart only scans if a scan is actually due -- not unconditionally, as
    // INDEX_ON_START used to.
    tasks::init(&*state.pool.get()?, config.index_on_start).context("initializing tasks")?;
    tasks::spawn_scheduler(state.clone());
    if !state.settings().configured() {
        info!("Media server not configured yet — open the Settings page to connect");
    }

    // index.html is served from a cache (re-read when the file changes) with the
    // URL base injected (see spa.rs);
    // every other path is a real static file, else the SPA's index.
    let index_page = Arc::new(spa::IndexPage::new(&config.web_dir, &config.url_base));
    let url_base = Arc::new(config.url_base.clone());
    // One handler for every "give me the page" case (unknown paths, `/base/`).
    let page = {
        let (idx, base) = (index_page.clone(), url_base.clone());
        move |axum::extract::OriginalUri(uri): axum::extract::OriginalUri| {
            let (idx, base) = (idx.clone(), base.clone());
            async move { spa::index_response(&idx.get(), &uri, &base) }
        }
    };
    let spa = ServeDir::new(&config.web_dir)
        .append_index_html_on_directories(false)
        .fallback(axum::Router::new().fallback(page.clone()));

    // Deliberately no CORS layer: the SPA is served from this same origin,
    // and Vite's dev server proxies /api server-side, so nothing legitimate
    // ever makes a cross-origin request. Leaving CORS headers off keeps the
    // browser's same-origin policy in force -- other sites' JS can't read
    // any response from this API, public endpoints included.
    let inner =
        api::router(state)
            .fallback_service(spa)
            .layer(axum::middleware::from_fn_with_state(
                (index_page.clone(), url_base.clone()),
                spa::serve_index,
            ));
    let app = if config.url_base.is_empty() {
        inner
    } else {
        // Hosted under a sub-path (URL_BASE): everything moves under it.
        // `/healthz` also stays at the container's root so the Dockerfile's
        // HEALTHCHECK (and anything probing the container directly) keeps
        // working.
        let base = config.url_base.clone();
        axum::Router::new()
            .route(
                "/healthz",
                axum::routing::get(|| async { axum::Json(serde_json::json!({ "ok": true })) }),
            )
            // axum 0.7 routes `/base` and `/base/*rest` into a nested router,
            // but not `/base/` itself -- the page's own URL is served here.
            .route(&format!("{base}/"), axum::routing::get(page))
            .nest(&base, inner)
    }
    .layer(TraceLayer::new_for_http());
    info!(
        "serving at {}",
        if config.url_base.is_empty() {
            "/"
        } else {
            &config.url_base
        }
    );

    let listener = tokio::net::TcpListener::bind(&config.bind_addr)
        .await
        .with_context(|| format!("binding {}", config.bind_addr))?;
    info!("listening on http://{}", config.bind_addr);

    // ConnectInfo gives handlers the real TCP peer address -- the login rate
    // limiter keys on it (see ratelimit.rs for why not X-Forwarded-For).
    let service = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
    if let Err(e) = axum::serve(listener, service).await {
        warn!("server error: {e:#}");
    }
    Ok(())
}

/// On first run, copy any env-provided Plex config into the DB so existing
/// env-based deployments keep working. Never overwrites values already saved
/// via the Settings page.
fn seed_settings_from_env(pool: &db::Pool, config: &Config) -> Result<()> {
    let mut conn = pool.get()?;
    let already = db::get_setting(&conn, "plex_url")?.is_some();
    if already {
        return Ok(());
    }
    if let Some(url) = &config.plex_url {
        db::set_setting(&conn, "plex_url", url)?;
    }
    if let Some(token) = &config.plex_token {
        db::set_setting(&conn, "plex_token", token)?;
    }
    if !config.path_map.is_empty() {
        let mappings: Vec<db::Mapping> = config
            .path_map
            .iter()
            .map(|r| db::Mapping {
                plex_prefix: r.from.clone(),
                container_path: r.to.clone(),
                label: None,
            })
            .collect();
        db::replace_mappings(&mut conn, &mappings)?;
    }
    Ok(())
}
