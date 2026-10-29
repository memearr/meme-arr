use super::*;

#[test]
fn row_count_tracks_largest_row_id_plus_one() {
    let mut idx = CompactIndex::new();
    assert_eq!(idx.row_count(), 0);
    idx.index_line(0, "hello");
    idx.index_line(1, "world");
    idx.index_line(5, "skip ahead");
    assert_eq!(idx.row_count(), 6);
}

#[test]
fn to_bytes_from_bytes_roundtrip_preserves_everything() {
    let mut idx = CompactIndex::with_block_size(4);
    idx.index_line(0, "I am the one who knocks");
    idx.index_line(1, "Say my name");
    idx.index_line(2, "I know who you are");
    idx.index_line(9, "A much later row");

    let bytes = idx.to_bytes();
    let loaded = CompactIndex::from_bytes(&bytes).unwrap();

    assert_eq!(loaded, idx);
    // And not just structurally equal — actually usable for queries.
    assert_eq!(
        loaded.query_candidate_blocks(&["name"]),
        idx.query_candidate_blocks(&["name"])
    );
}

#[test]
fn save_to_load_from_roundtrips_via_file() {
    let mut idx = CompactIndex::with_block_size(32);
    for i in 0..50u32 {
        idx.index_line(i, &format!("row number {i} says hello"));
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("compact_index.bin");
    idx.save_to(&path).unwrap();
    // save_to must leave no .tmp file behind on success.
    assert!(!path.with_extension("tmp").exists());

    let loaded = CompactIndex::load_from(&path).unwrap();
    assert_eq!(loaded, idx);
}

#[test]
fn from_bytes_rejects_bad_magic() {
    let err = CompactIndex::from_bytes(b"NOTAVALIDHEADERATALL").unwrap_err();
    assert!(err.to_string().contains("bad magic"));
}

#[test]
fn from_bytes_rejects_truncated_buffer() {
    // A valid buffer with real content, truncated partway through its
    // postings section -- exercises the bounds checks, not the magic
    // check above.
    let mut idx = CompactIndex::new();
    idx.index_line(0, "a line with some real content in it");
    let full = idx.to_bytes();
    let truncated = &full[..full.len() - 3];
    assert!(CompactIndex::from_bytes(truncated).is_err());
}

#[test]
fn from_bytes_rejects_wrong_format_version() {
    let idx = CompactIndex::new();
    let mut bytes = idx.to_bytes();
    // Format version is the 4 bytes right after the magic.
    let version_start = COMPACT_INDEX_MAGIC.len();
    bytes[version_start..version_start + 4].copy_from_slice(&999u32.to_le_bytes());
    let err = CompactIndex::from_bytes(&bytes).unwrap_err();
    assert!(err
        .to_string()
        .contains("unsupported compact index format version"));
}

#[test]
fn rebuild_from_db_matches_a_manual_build() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE subtitle_cue (id INTEGER PRIMARY KEY, text TEXT NOT NULL)")
        .unwrap();
    let lines = [
        "I am the one who knocks",
        "Say my name",
        "I know who you are",
    ];
    for line in &lines {
        conn.execute("INSERT INTO subtitle_cue(text) VALUES (?1)", [line])
            .unwrap();
    }

    let rebuilt = CompactIndex::rebuild_from_db(&conn, 1).unwrap();

    let mut manual = CompactIndex::with_block_size(1);
    for (i, line) in lines.iter().enumerate() {
        // subtitle_cue ids are 1-based (SQLite INTEGER PRIMARY KEY
        // autoincrement starting at 1), matching rebuild_from_db's
        // direct use of the real `id` column as the row id.
        manual.index_line(i as u32 + 1, line);
    }

    assert_eq!(rebuilt.row_count(), manual.row_count());
    assert_eq!(
        rebuilt.query_candidate_blocks(&["who"]),
        manual.query_candidate_blocks(&["who"])
    );
    assert_eq!(
        rebuilt.query_candidate_blocks(&["name"]),
        manual.query_candidate_blocks(&["name"])
    );
}

#[test]
fn rice_roundtrips_for_various_k() {
    let deltas = vec![1u32, 1, 2, 1, 5, 100, 1, 1, 3, 4096, 1];
    for k in 0..16 {
        let encoded = rice_encode(&deltas, k);
        let decoded = rice_decode(&encoded, k, deltas.len());
        assert_eq!(decoded, deltas, "mismatch at k={k}");
    }
}

#[test]
fn rice_roundtrips_zero_delta() {
    // Rice coding itself is fine with a zero value even though
    // `Postings` never stores one (ids are strictly increasing).
    let deltas = vec![0u32, 0, 1, 0];
    let k = 0;
    let encoded = rice_encode(&deltas, k);
    assert_eq!(rice_decode(&encoded, k, deltas.len()), deltas);
}

#[test]
fn optimal_rice_k_grows_with_mean() {
    assert_eq!(optimal_rice_k(1.0), 0);
    assert!(optimal_rice_k(100.0) > optimal_rice_k(10.0));
    assert!(optimal_rice_k(10_000.0) > optimal_rice_k(100.0));
}

#[test]
fn rice_size_beats_varint_for_small_skewed_deltas() {
    // A very common term: mostly delta=1 (consecutive rows), i.e.
    // exactly the "small value, byte-aligned encoding wastes padding"
    // case Rice coding exists to fix.
    let mut idx = CompactIndex::new();
    for row in 0..2000u32 {
        idx.index_line(row, "the quick fox");
    }
    let varint_bytes = idx.size_bytes();
    let rice_bytes = idx.rice_encoded_size_bytes();
    assert!(
        rice_bytes < varint_bytes,
        "rice ({rice_bytes}) should beat varint ({varint_bytes}) for small, skewed deltas"
    );
}

#[test]
fn varint_roundtrips() {
    for v in [0u64, 1, 127, 128, 300, 16384, u32::MAX as u64] {
        let mut buf = Vec::new();
        encode_varint(v, &mut buf);
        let mut pos = 0;
        assert_eq!(decode_varint(&buf, &mut pos), v);
        assert_eq!(pos, buf.len());
    }
}

#[test]
fn postings_roundtrip_in_order() {
    let mut p = Postings::default();
    for id in [1u32, 5, 6, 100, 101, 50_000] {
        p.append(id);
    }
    let got: Vec<u32> = p.iter().collect();
    assert_eq!(got, vec![1, 5, 6, 100, 101, 50_000]);
    assert_eq!(p.len(), 6);
}

#[test]
fn small_deltas_cost_roughly_one_byte() {
    // Dense, closely-spaced ids (the common case for a frequent word)
    // should compress to close to 1 byte/posting, not grow unbounded.
    let mut p = Postings::default();
    for id in 0..10_000u32 {
        p.append(id * 2); // delta of 2 every time
    }
    let bytes_per_posting = p.size_bytes() as f64 / p.len() as f64;
    assert!(
        bytes_per_posting < 1.5,
        "got {bytes_per_posting} bytes/posting"
    );
}

#[test]
fn intersect_finds_common_ids() {
    let mut a = Postings::default();
    let mut b = Postings::default();
    let mut c = Postings::default();
    for id in [1u32, 2, 3, 4, 5] {
        a.append(id);
    }
    for id in [2u32, 3, 5, 7] {
        b.append(id);
    }
    for id in [3u32, 5, 9] {
        c.append(id);
    }
    assert_eq!(intersect(&[&a, &b]), vec![2, 3, 5]);
    assert_eq!(intersect(&[&a, &b, &c]), vec![3, 5]);
}

#[test]
fn intersect_empty_list_is_empty() {
    let a = Postings::default();
    let mut b = Postings::default();
    b.append(1);
    assert!(intersect(&[&a, &b]).is_empty());
    assert!(intersect(&[]).is_empty());
}

#[test]
fn tokenize_lowercases_and_splits_on_punctuation() {
    assert_eq!(
        tokenize("I am the one who knocks."),
        vec!["i", "am", "the", "one", "who", "knocks"]
    );
    assert_eq!(
        tokenize("Don't!! stop--now"),
        vec!["don", "t", "stop", "now"]
    );
}

#[test]
fn index_and_query_roundtrip() {
    let mut idx = CompactIndex::new();
    idx.index_line(0, "I am the one who knocks");
    idx.index_line(1, "Say my name");
    idx.index_line(2, "I know who you are");

    let hits = idx.query_candidate_blocks(&["knocks"]);
    assert_eq!(hits, vec![0]);

    let hits = idx.query_candidate_blocks(&["who"]);
    assert_eq!(hits, vec![0, 2]); // "who" appears in rows 0 and 2

    let hits = idx.query_candidate_blocks(&["name"]);
    assert_eq!(hits, vec![1]);

    let hits = idx.query_candidate_blocks(&["nonexistent"]);
    assert!(hits.is_empty());
}

#[test]
fn stopwords_are_indexed_but_excluded_from_the_hot_path() {
    let mut idx = CompactIndex::new();
    idx.index_line(0, "the one who knocks");
    idx.index_line(1, "the quick fox");
    // "the" IS in the dictionary (needed for the all-stopword fallback)...
    assert!(idx.term_to_id.contains_key("the"));
    // ...but a mixed query only narrows on the real term, same result
    // whether or not the stopword is included.
    assert_eq!(idx.query_candidate_blocks(&["the", "knocks"]), vec![0]);
    assert_eq!(idx.query_candidate_blocks(&["knocks"]), vec![0]);
}

#[test]
fn all_stopword_query_falls_back_instead_of_returning_nothing() {
    let mut idx = CompactIndex::new();
    idx.index_line(0, "to be or not to be");
    idx.index_line(1, "something else entirely");
    // Every term in this query is a stopword, so the normal "exclude
    // stopwords" path would leave zero terms; confirm the fallback finds
    // the right row instead of wrongly returning nothing.
    let hits = idx.query_candidate_blocks(&["to", "be"]);
    assert_eq!(hits, vec![0]);
}

#[test]
fn phrase_verification_rejects_out_of_order_matches() {
    let terms = vec!["one".to_string(), "who".to_string(), "knocks".to_string()];
    assert!(phrase_matches("I am the one who knocks", &terms));
    assert!(!phrase_matches("who knocks is not the one", &terms));
    assert!(!phrase_matches("the one who definitely knocks", &terms));
}

#[test]
fn appending_out_of_order_id_panics_in_debug() {
    let mut p = Postings::default();
    p.append(5);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        p.append(3);
    }));
    assert!(result.is_err());
}
