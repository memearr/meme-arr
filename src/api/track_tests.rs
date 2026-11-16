use super::*;

fn body(over: impl FnOnce(&mut TrackBody)) -> TrackBody {
    let mut b = TrackBody {
        rating_key: "nope".into(),
        start_ms: 1000,
        end_ms: 5000,
        t_ms: 0,
        x: 0.5,
        y: 0.5,
        size: 0.12,
        max_points: 100,
    };
    over(&mut b);
    b
}

async fn call(b: TrackBody) -> (StatusCode, String) {
    let st = crate::state::test_state();
    let resp = track_spot(State(st), Json(b)).await.ok().unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

#[tokio::test]
async fn a_request_that_makes_no_sense_is_refused_before_anything_is_read() {
    for (b, why) in [
        (body(|b| b.x = f32::NAN), "fractions of the picture"),
        (body(|b| b.y = 1.5), "fractions of the picture"),
        (body(|b| b.x = -0.1), "fractions of the picture"),
        (body(|b| b.size = f32::INFINITY), "fractions of the picture"),
        (body(|b| b.end_ms = b.start_ms), "end must be greater"),
        (body(|b| b.end_ms = b.start_ms + 61_000), "60 seconds"),
        (body(|b| b.t_ms = -1), "within the clip"),
        (body(|b| b.t_ms = 4001), "within the clip"),
    ] {
        let (status, text) = call(b).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{why}: {text}");
        assert!(text.contains(why), "{why}: {text}");
    }
}

#[tokio::test]
async fn media_that_is_not_indexed_is_not_found() {
    let (status, text) = call(body(|_| {})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{text}");
}

#[test]
fn the_body_reads_with_defaults() {
    let b: TrackBody =
        serde_json::from_str(r#"{"rating_key":"7","start_ms":0,"end_ms":3000,"x":0.4,"y":0.6}"#)
            .unwrap();
    assert_eq!((b.t_ms, b.size, b.max_points), (0, 0.12, 100));
}
