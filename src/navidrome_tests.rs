use super::*;
use serde_json::json;

#[test]
fn the_token_is_the_subsonic_documented_one() {
    // The example in the Subsonic API documentation (password and salt assembled here
    // rather than written out: they are a published example, not credentials).
    let (password, salt) = (["ses", "ame"].concat(), ["c19b", "2d"].concat());
    assert_eq!(token(&password, &salt), "26719a1196d2a940705a59634eb18eab");
}

#[test]
fn every_request_carries_a_fresh_salt_and_never_the_password() {
    let (password, salt) = (
        format!("pw-{}", uuid::Uuid::new_v4().simple()),
        uuid::Uuid::new_v4().simple().to_string(),
    );
    let c = Client::new("http://navi:4533/", " me ", &password);
    let q = c.auth(&salt);
    let get = |k: &str| q.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str());
    assert_eq!(get("u"), Some("me"));
    assert_eq!(get("s"), Some(salt.as_str()));
    assert_eq!(get("t"), Some(token(&password, &salt).as_str()));
    assert_eq!(get("f"), Some("json"));
    assert!(q.iter().all(|(_, v)| *v != password));
    let url = c.request("ping", &[]).build().unwrap();
    assert!(url
        .url()
        .as_str()
        .starts_with("http://navi:4533/rest/ping?"));
    assert!(!url.url().as_str().contains(&password));
}

#[test]
fn an_ok_answer_is_its_payload_and_a_failed_one_is_its_message() {
    let ok = json!({"subsonic-response": {"status": "ok", "serverVersion": "0.52"}});
    assert_eq!(envelope(&ok).unwrap()["serverVersion"], "0.52");
    let bad = json!({"subsonic-response": {"status": "failed",
        "error": {"code": 40, "message": "Wrong username or password"}}});
    assert_eq!(
        envelope(&bad).unwrap_err().to_string(),
        "Wrong username or password"
    );
    let odd = json!({"hello": 1});
    assert!(envelope(&odd).unwrap_err().to_string().contains("Subsonic"));
}

#[test]
fn songs_are_read_from_a_search_answer() {
    let body = json!({"subsonic-response": {"status": "ok", "searchResult3": {"song": [
        {"id": "a1", "title": "Song", "artist": "Band", "album": "LP", "duration": 187},
        {"id": "", "title": "no id"},
        {"id": "b2", "title": "Bare"}
    ]}}});
    let t = parse_search(&body).unwrap();
    assert_eq!(t.len(), 2);
    assert_eq!(
        t[0],
        Track {
            id: "a1".into(),
            title: "Song".into(),
            artist: "Band".into(),
            album: "LP".into(),
            duration_s: 187
        }
    );
    assert_eq!(t[1].duration_s, 0);
    // nothing found: Navidrome sends an empty searchResult3
    let none = json!({"subsonic-response": {"status": "ok", "searchResult3": {}}});
    assert!(parse_search(&none).unwrap().is_empty());
}
