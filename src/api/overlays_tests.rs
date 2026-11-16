use super::*;

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

/// A state whose data folder is a fresh temp one.
fn state() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let mut st = crate::state::test_state();
    let mut cfg = (*st.config).clone();
    cfg.data_dir = dir.path().to_path_buf();
    st.config = std::sync::Arc::new(cfg);
    (dir, st)
}

async fn body(resp: Response) -> (StatusCode, Vec<u8>) {
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .ok()
        .unwrap();
    (status, bytes.to_vec())
}

fn json_of(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap_or(serde_json::Value::Null)
}

async fn upload(st: &AppState, name: &str, bytes: &[u8]) -> (StatusCode, serde_json::Value) {
    let resp = upload_overlay(
        State(st.clone()),
        Query(OverlayUpload { name: name.into() }),
        Bytes::copy_from_slice(bytes),
    )
    .await
    .ok()
    .unwrap();
    let (status, b) = body(resp).await;
    (status, json_of(&b))
}

#[tokio::test]
async fn an_upload_is_listed_served_and_deleted() {
    let (_d, st) = state();
    let listed = list_overlays(State(st.clone()))
        .await
        .ok()
        .unwrap()
        .into_response();
    let (_, b) = body(listed).await;
    let j = json_of(&b);
    assert_eq!(j["overlays"].as_array().unwrap().len(), 0);
    // the built-in shapes are always offered
    let shapes: Vec<_> = j["shapes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(shapes, ["arrow", "ring", "box", "dot"]);

    let (status, j) = upload(&st, "my sticker.png", PNG).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = j["overlay"]["id"].as_str().unwrap().to_string();
    assert_eq!(j["overlay"]["kind"], "still");

    let listed = list_overlays(State(st.clone()))
        .await
        .ok()
        .unwrap()
        .into_response();
    let (_, b) = body(listed).await;
    assert_eq!(json_of(&b)["overlays"][0]["id"], id.as_str());

    // the file comes back as it went in, with the right type
    let resp = overlay_file(State(st.clone()), Path(id.clone()), HeaderMap::new())
        .await
        .ok()
        .unwrap();
    assert_eq!(resp.headers()[header::CONTENT_TYPE], "image/png");
    assert_eq!(body(resp).await.1, PNG);

    let gone = delete_overlay(State(st.clone()), Path(id.clone()))
        .await
        .ok()
        .unwrap();
    assert_eq!(gone.status(), StatusCode::NO_CONTENT);
    let again = delete_overlay(State(st.clone()), Path(id.clone()))
        .await
        .ok()
        .unwrap();
    assert_eq!(again.status(), StatusCode::NOT_FOUND);
    let resp = overlay_file(State(st), Path(id), HeaderMap::new())
        .await
        .ok()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bad_uploads_get_the_right_status() {
    let (_d, st) = state();
    assert_eq!(upload(&st, "a.png", b"").await.0, StatusCode::BAD_REQUEST);
    let (status, j) = upload(&st, "a.png", b"#!/bin/sh\n").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(j["error"].as_str().unwrap().contains("PNG"));
    let mut big = PNG.to_vec();
    big.resize(crate::overlays::MAX_OVERLAY_BYTES + 1, 0);
    assert_eq!(
        upload(&st, "a.png", &big).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn a_path_cannot_climb_out_of_the_overlay_folder() {
    let (d, st) = state();
    // a file next to the overlay folder that a traversal would reach
    std::fs::write(d.path().join("secret.png"), PNG).unwrap();
    for bad in ["../secret.png", "..\\secret.png", "a/../../secret.png"] {
        let resp = overlay_file(State(st.clone()), Path(bad.into()), HeaderMap::new())
            .await
            .ok()
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{bad}");
        let gone = delete_overlay(State(st.clone()), Path(bad.into()))
            .await
            .ok()
            .unwrap();
        assert_eq!(gone.status(), StatusCode::NOT_FOUND, "{bad}");
    }
    assert!(d.path().join("secret.png").exists());
}

#[tokio::test]
async fn a_shape_sprite_is_a_png_in_the_asked_colour() {
    let resp = shape_sprite(
        Path("arrow.png".into()),
        Query(SpriteParams {
            color: Some("#00ff00".into()),
        }),
    )
    .await
    .ok()
    .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()[header::CONTENT_TYPE], "image/png");
    let (_, png) = body(resp).await;
    assert_eq!(&png[1..4], b"PNG");
    // the default colour is a different picture
    let orange = shape_sprite(Path("arrow".into()), Query(SpriteParams { color: None }))
        .await
        .ok()
        .unwrap();
    assert_ne!(body(orange).await.1, png);

    let unknown = shape_sprite(Path("triangle".into()), Query(SpriteParams { color: None }))
        .await
        .ok()
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    let bad = shape_sprite(
        Path("arrow".into()),
        Query(SpriteParams {
            color: Some("red".into()),
        }),
    )
    .await
    .ok()
    .unwrap();
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
}
