//! An in-process client for the real router, shared by the handler tests: the same routes, auth
//! middleware and error mapping the server runs, without opening a socket.
#![allow(dead_code)] // a shared kit: each test module uses the parts it needs

use super::router;
use crate::state::{test_state, AppState};
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{header, HeaderMap, Method, Request, StatusCode},
    Router,
};
use serde_json::Value;
use std::net::SocketAddr;
use tower::ServiceExt;

/// What came back.
pub(crate) struct Resp {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Resp {
    /// The body as JSON (`Null` when it isn't).
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The `error` message of a JSON error body.
    pub fn error(&self) -> String {
        self.json()["error"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }
}

pub(crate) struct Harness {
    pub dir: tempfile::TempDir,
    pub state: AppState,
    app: Router,
    cookie: Option<String>,
    /// The admin password `sign_in` set.
    pub password: String,
}

/// A password made for one test (never a literal).
pub(crate) fn secret() -> String {
    format!("pw-{}", uuid::Uuid::new_v4().simple())
}

impl Harness {
    /// A fresh app in a temp folder, with nobody signed in and no admin password set yet.
    pub fn bare() -> Self {
        Self::with_state(|_| {})
    }

    /// Like `bare`, adjusting the config first (before the router captures it).
    pub fn with_state(tweak: impl FnOnce(&mut crate::config::Config)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut state = test_state();
        let mut cfg = (*state.config).clone();
        cfg.data_dir = dir.path().join("data");
        cfg.output_dir = dir.path().join("output");
        std::fs::create_dir_all(&cfg.data_dir).unwrap();
        std::fs::create_dir_all(&cfg.output_dir).unwrap();
        cfg.browse_roots = vec![dir.path().to_path_buf()];
        tweak(&mut cfg);
        state.config = std::sync::Arc::new(cfg);
        let app = router(state.clone());
        Harness {
            dir,
            state,
            app,
            cookie: None,
            password: String::new(),
        }
    }

    /// A signed-in app: first-run setup done and the session cookie kept for later calls.
    pub async fn new() -> Self {
        let mut h = Self::bare();
        h.sign_in().await;
        h
    }

    /// Set the admin password and keep the session cookie that comes back.
    pub async fn sign_in(&mut self) {
        self.password = secret();
        let r = self
            .call(
                Method::POST,
                "/api/auth/setup",
                Some(serde_json::json!({"password": self.password})),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "setup failed: {}", r.text());
        let set = r.headers.get(header::SET_COOKIE).unwrap().to_str().unwrap();
        self.cookie = Some(set.split(';').next().unwrap().to_string());
    }

    /// Send a request (JSON body when given), with the session cookie if there is one.
    pub async fn call(&self, method: Method, uri: &str, body: Option<Value>) -> Resp {
        let mut b = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(v) => {
                b = b.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        self.send(b, body).await
    }

    /// Send a request with a raw body of the given content type.
    pub async fn call_raw(
        &self,
        method: Method,
        uri: &str,
        content_type: &str,
        body: Vec<u8>,
    ) -> Resp {
        let b = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, content_type);
        self.send(b, Body::from(body)).await
    }

    /// Send a request that carries extra headers.
    pub async fn call_with(&self, method: Method, uri: &str, headers: &[(&str, &str)]) -> Resp {
        let mut b = Request::builder().method(method).uri(uri);
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        self.send(b, Body::empty()).await
    }

    async fn send(&self, mut b: axum::http::request::Builder, body: Body) -> Resp {
        if let Some(c) = &self.cookie {
            b = b.header(header::COOKIE, c.as_str());
        }
        let mut req = b.body(body).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 5555))));
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec();
        Resp {
            status,
            headers,
            body,
        }
    }

    pub async fn get(&self, uri: &str) -> Resp {
        self.call(Method::GET, uri, None).await
    }

    pub async fn post(&self, uri: &str, body: Value) -> Resp {
        self.call(Method::POST, uri, Some(body)).await
    }

    pub async fn put(&self, uri: &str, body: Value) -> Resp {
        self.call(Method::PUT, uri, Some(body)).await
    }

    pub async fn delete(&self, uri: &str) -> Resp {
        self.call(Method::DELETE, uri, None).await
    }
}

impl Harness {
    /// Three libraries (TV, Anime, Movies) with `per_library` items each; every item has the
    /// line "knock knock who is there".
    pub async fn seed(&self, per_library: usize) {
        self.state
            .db(move |c| {
                crate::db::seed_libraries(c, per_library);
                Ok(())
            })
            .await
            .unwrap();
    }
}
