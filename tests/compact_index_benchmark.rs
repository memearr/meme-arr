//! Real, measured comparison of `compactindex::CompactIndex` against actual
//! SQLite FTS5 tables (`detail=full`, the current production setting, and
//! `detail=none`), over synthetic dialogue-shaped text and, when available,
//! a real slice of the user's own Plex dialogue index.
//!
//! This is a benchmark, not a correctness gate — it's marked `#[ignore]` so
//! normal `cargo test` stays fast. Run it explicitly:
//!
//!     cargo test --release --test compact_index_benchmark -- --ignored --nocapture
//!
//! Caveat on the synthetic-corpus test: it approximates real subtitle text's
//! statistical shape (a Zipfian-weighted vocabulary, short lines) but places
//! terms uniformly at random — real dialogue clusters by show/episode, which
//! it can't model. `compact_index_vs_fts5_benchmark_real_data` exists to
//! check whether that difference actually moves the measured ratios; it
//! reads `real_sample.txt` from the repo root (untracked — real copyrighted
//! dialogue, one subtitle cue's text per line, in original row order) and
//! skips itself with a message if that file isn't present.

use meme_arr::compactindex::{phrase_matches, tokenize, CompactIndex};
use rusqlite::Connection;
use std::time::Instant;

/// A modest vocabulary mixing common "stopword-ish" function words (sampled
/// heavily, like real language) with a wider pool of content words (sampled
/// with a rough power-law skew), enough to produce realistically-repetitive
/// short lines without needing an external word-list dependency.
fn vocab() -> (Vec<&'static str>, Vec<&'static str>) {
    let function_words = vec![
        "the", "a", "an", "and", "are", "as", "at", "be", "been", "but", "by", "do", "for", "from",
        "had", "has", "have", "he", "her", "his", "i", "in", "is", "it", "its", "me", "my", "no",
        "not", "of", "on", "or", "our", "she", "so", "that", "the", "their", "there", "they",
        "this", "to", "was", "we", "were", "will", "with", "would", "you", "your",
    ];
    let content_words = vec![
        "know",
        "want",
        "going",
        "right",
        "time",
        "think",
        "just",
        "like",
        "good",
        "get",
        "see",
        "come",
        "take",
        "make",
        "go",
        "tell",
        "look",
        "need",
        "mean",
        "way",
        "day",
        "man",
        "life",
        "world",
        "people",
        "thing",
        "work",
        "home",
        "money",
        "name",
        "place",
        "year",
        "night",
        "door",
        "car",
        "house",
        "father",
        "mother",
        "friend",
        "love",
        "kill",
        "die",
        "dead",
        "gun",
        "police",
        "stop",
        "wait",
        "sorry",
        "please",
        "okay",
        "yeah",
        "yes",
        "sir",
        "boss",
        "baby",
        "girl",
        "boy",
        "guy",
        "woman",
        "kid",
        "doctor",
        "hospital",
        "school",
        "city",
        "street",
        "room",
        "phone",
        "call",
        "talk",
        "listen",
        "hear",
        "say",
        "said",
        "told",
        "ask",
        "answer",
        "question",
        "problem",
        "deal",
        "plan",
        "idea",
        "truth",
        "lie",
        "secret",
        "promise",
        "trust",
        "fight",
        "run",
        "walk",
        "drive",
        "move",
        "leave",
        "stay",
        "back",
        "forward",
        "left",
        "now",
        "then",
        "today",
        "tomorrow",
        "never",
        "always",
        "sometimes",
        "maybe",
        "probably",
        "actually",
        "really",
        "sure",
        "fine",
        "great",
        "bad",
        "wrong",
        "crazy",
        "stupid",
        "smart",
        "beautiful",
        "strong",
        "weak",
        "big",
        "small",
        "long",
        "short",
        "old",
        "new",
        "first",
        "last",
        "next",
        "same",
        "different",
        "important",
        "dangerous",
        "safe",
        "happy",
        "sad",
        "angry",
        "scared",
        "worried",
        "tired",
        "knocks",
        "flow",
        "spice",
        "breaking",
        "bad",
        "chemistry",
        "cook",
        "empire",
        "family",
        "respect",
        "power",
        "danger",
        "risk",
    ];
    (function_words, content_words)
}

/// Deterministic, dependency-free PRNG (xorshift) so the benchmark is
/// reproducible without pulling in the `rand` crate for a test-only tool.
struct Xorshift(u64);
impl Xorshift {
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn next_range(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// Generate `n` dialogue-shaped lines: 4-12 words, roughly 70% drawn from the
/// (small, heavily-repeated) function-word pool and 30% from the wider
/// content-word pool — a rough approximation of real spoken dialogue's
/// skew toward a small set of very common words.
fn generate_lines(n: usize, seed: u64) -> Vec<String> {
    let (function_words, content_words) = vocab();
    let mut rng = Xorshift(seed);
    let mut lines = Vec::with_capacity(n);
    for _ in 0..n {
        let len = 4 + rng.next_range(9);
        let mut words = Vec::with_capacity(len);
        for _ in 0..len {
            let pick_function = rng.next_range(10) < 7;
            let word = if pick_function {
                function_words[rng.next_range(function_words.len())]
            } else {
                content_words[rng.next_range(content_words.len())]
            };
            words.push(word);
        }
        // Capitalize the first word, loosely mimicking real subtitle casing.
        let mut line = words.join(" ");
        if let Some(c) = line.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        line.push('.');
        lines.push(line);
    }
    lines
}

fn build_compact_index(lines: &[String], block_size: u32) -> CompactIndex {
    let mut idx = CompactIndex::with_block_size(block_size);
    for (i, line) in lines.iter().enumerate() {
        idx.index_line(i as u32, line);
    }
    idx
}

/// Build a real, on-disk SQLite FTS5 table over `lines` with the given
/// `detail` setting, and return its file size in bytes.
/// Create and populate just the plain content table (no FTS5 at all) — the
/// baseline we diff against to isolate the FTS5 index's own footprint,
/// since an external-content FTS5 table (what production actually uses,
/// `content='subtitle_cue'`) stores no second copy of the text itself.
fn build_content_table(db_path: &std::path::Path, lines: &[String]) -> u64 {
    let mut conn = Connection::open(db_path).unwrap();
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
        .unwrap();
    conn.execute(
        "CREATE TABLE cue (id INTEGER PRIMARY KEY, text TEXT NOT NULL)",
        [],
    )
    .unwrap();
    {
        let tx = conn.transaction().unwrap();
        {
            let mut stmt = tx.prepare("INSERT INTO cue(text) VALUES (?1)").unwrap();
            for line in lines {
                stmt.execute([line]).unwrap();
            }
        }
        tx.commit().unwrap();
    }
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    drop(conn);
    std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0)
}

/// Same content table, plus an **external-content** FTS5 index over it —
/// matching production's real schema (`subtitle_fts`/`subtitle_cue`) exactly,
/// rather than a self-contained FTS5 table that would store its own
/// redundant copy of the text. Returns the total file size; subtract
/// `build_content_table`'s result from this to get the FTS5 index's own
/// footprint, cleanly comparable to `CompactIndex::size_bytes()`.
fn build_fts5_external(db_path: &std::path::Path, lines: &[String], detail: &str) -> u64 {
    let mut conn = Connection::open(db_path).unwrap();
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
        .unwrap();
    conn.execute(
        "CREATE TABLE cue (id INTEGER PRIMARY KEY, text TEXT NOT NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        &format!(
            "CREATE VIRTUAL TABLE fts USING fts5(text, content='cue', content_rowid='id', detail='{detail}')"
        ),
        [],
    )
    .unwrap();
    {
        let tx = conn.transaction().unwrap();
        {
            let mut cue_stmt = tx.prepare("INSERT INTO cue(text) VALUES (?1)").unwrap();
            let mut fts_stmt = tx
                .prepare("INSERT INTO fts(rowid, text) VALUES (last_insert_rowid(), ?1)")
                .unwrap();
            for line in lines {
                cue_stmt.execute([line]).unwrap();
                fts_stmt.execute([line]).unwrap();
            }
        }
        tx.commit().unwrap();
    }
    conn.execute_batch("INSERT INTO fts(fts) VALUES('optimize'); PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    drop(conn);
    std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0)
}

fn fmt_bytes(n: u64) -> String {
    let n = n as f64;
    if n < 1024.0 {
        return format!("{n:.0} B");
    }
    let units = ["KB", "MB", "GB"];
    let mut v = n / 1024.0;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.2} {}", units[i])
}

/// Run the fixed query set against both the compact index and FTS5, asserting
/// they agree on the exact set of matching rows, and return
/// `(avg_compact_query_us, avg_fts5_query_us)`.
///
/// For `block_size > 1` a candidate is a *block id*, not a row id — it names
/// a superset of `block_size` consecutive rows, any of which might be the
/// real match (or none, if the block was only a false-positive candidate).
/// So verification must expand every candidate block into its row range via
/// `block_range()` and phrase-check each row in it, rather than checking the
/// candidate id directly — that direct check is only valid when
/// `block_size == 1`, where a block *is* a row.
fn run_queries(
    compact: &CompactIndex,
    lines: &[String],
    fts5_conn: &Connection,
    queries: &[&str],
) -> (f64, f64) {
    let mut compact_query_ns = Vec::new();
    let mut fts5_query_ns = Vec::new();

    for q in queries {
        let terms: Vec<String> = tokenize(q);
        let term_refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();

        let t0 = Instant::now();
        let candidate_blocks = compact.query_candidate_blocks(&term_refs);
        let mut verified: Vec<usize> = Vec::new();
        for block_id in candidate_blocks {
            let range = compact.block_range(block_id);
            for row in range {
                let row = row as usize;
                if row < lines.len() && phrase_matches(&lines[row], &terms) {
                    verified.push(row);
                }
            }
        }
        verified.sort_unstable();
        verified.dedup();
        compact_query_ns.push(t0.elapsed().as_nanos());

        let t0 = Instant::now();
        let fts5_hits: Vec<i64> = fts5_conn
            .prepare("SELECT rowid FROM fts WHERE fts MATCH ?1 ORDER BY rowid")
            .unwrap()
            .query_map([format!("\"{q}\"")], |r| r.get::<_, i64>(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        fts5_query_ns.push(t0.elapsed().as_nanos());
        // FTS5 rowids are 1-based and line up with insertion order.
        let mut fts5_rows: Vec<usize> = fts5_hits.iter().map(|&r| (r - 1) as usize).collect();
        fts5_rows.sort_unstable();

        assert_eq!(
            verified,
            fts5_rows,
            "compact index and FTS5 disagree for query {q:?} (block_size={})",
            compact.block_size()
        );
    }

    let avg_compact_us: f64 =
        compact_query_ns.iter().sum::<u128>() as f64 / compact_query_ns.len() as f64 / 1000.0;
    let avg_fts5_us: f64 =
        fts5_query_ns.iter().sum::<u128>() as f64 / fts5_query_ns.len() as f64 / 1000.0;
    (avg_compact_us, avg_fts5_us)
}

/// The full size + correctness + Rice-coding + block-size sweep, shared
/// between the synthetic-corpus benchmark and the real-data one — same
/// methodology, different input, so the two are actually comparable.
fn run_full_benchmark(label: &str, lines: &[String], queries: &[&str]) {
    let n = lines.len();
    eprintln!("\n=== compact index vs. SQLite FTS5 — {label} ({n} lines) ===");

    // --- Build everything ---
    let t0 = Instant::now();
    let compact = build_compact_index(lines, 1);
    let compact_build_ms = t0.elapsed().as_millis();
    let compact_bytes = compact.size_bytes() as u64;

    let dir = tempfile::tempdir().unwrap();
    // Baseline: the content table alone, no FTS5 at all — subtracting this
    // from the FTS5-table totals below isolates the index's own footprint,
    // since external-content FTS5 (what production uses) stores no second
    // copy of the text itself.
    let content_only_bytes = build_content_table(&dir.path().join("content.sqlite3"), lines);

    let t0 = Instant::now();
    let fts5_full_total = build_fts5_external(&dir.path().join("full.sqlite3"), lines, "full");
    let fts5_full_build_ms = t0.elapsed().as_millis();
    let fts5_full_index_bytes = fts5_full_total.saturating_sub(content_only_bytes);

    let t0 = Instant::now();
    let fts5_none_total = build_fts5_external(&dir.path().join("none.sqlite3"), lines, "none");
    let fts5_none_build_ms = t0.elapsed().as_millis();
    let fts5_none_index_bytes = fts5_none_total.saturating_sub(content_only_bytes);

    // Raw text size, for context (what the content table itself holds).
    let raw_text_bytes: u64 = lines.iter().map(|l| l.len() as u64).sum();

    eprintln!(
        "raw text:              {:>10}  ({:.2} bytes/row)",
        fmt_bytes(raw_text_bytes),
        raw_text_bytes as f64 / n as f64
    );
    eprintln!(
        "content table (no idx):{:>10}  ({:.2} bytes/row)  — baseline, subtracted below",
        fmt_bytes(content_only_bytes),
        content_only_bytes as f64 / n as f64
    );
    eprintln!(
        "compact index:         {:>10}  ({:.2} bytes/row)  [build: {compact_build_ms}ms]  terms={}",
        fmt_bytes(compact_bytes),
        compact_bytes as f64 / n as f64,
        compact.term_count(),
    );
    eprintln!(
        "FTS5 index, detail=full:{:>10}  ({:.2} bytes/row)  [build: {fts5_full_build_ms}ms]  (current production setting; index-only, text excluded)",
        fmt_bytes(fts5_full_index_bytes),
        fts5_full_index_bytes as f64 / n as f64,
    );
    eprintln!(
        "FTS5 index, detail=none:{:>10}  ({:.2} bytes/row)  [build: {fts5_none_build_ms}ms]  (index-only, text excluded)",
        fmt_bytes(fts5_none_index_bytes),
        fts5_none_index_bytes as f64 / n as f64,
    );
    eprintln!(
        "compact vs FTS5 detail=full (index-only): {:.1}% of the size",
        compact_bytes as f64 / fts5_full_index_bytes as f64 * 100.0
    );
    eprintln!(
        "compact vs FTS5 detail=none (index-only): {:.1}% of the size",
        compact_bytes as f64 / fts5_none_index_bytes as f64 * 100.0
    );

    // --- Rice coding: a complementary, query-time-free size win. Unlike the
    // block-size sweep below, re-encoding postings bytes doesn't change what
    // a query has to scan or verify at all — it only changes how compactly
    // the same deltas are stored, so query latency is untouched.
    let rice_bytes = compact.rice_encoded_size_bytes() as u64;
    eprintln!(
        "compact index, Rice-coded postings: {:>10}  ({:.2} bytes/row)",
        fmt_bytes(rice_bytes),
        rice_bytes as f64 / n as f64
    );
    eprintln!(
        "Rice vs varint (same compact index, same query behavior): {:.1}% of the size",
        rice_bytes as f64 / compact_bytes as f64 * 100.0
    );
    eprintln!(
        "Rice vs FTS5 detail=full (index-only): {:.1}% of the size",
        rice_bytes as f64 / fts5_full_index_bytes as f64 * 100.0
    );

    // --- Correctness: does the compact index agree with FTS5's own native
    // phrase match, for every query? FTS5 (detail=full, the current
    // production setting) is the ground-truth oracle here — not a hand-picked
    // "expected" row list, since an injected or picked marker phrase can
    // plausibly recur by pure chance elsewhere in the corpus too. The
    // property that actually matters is "do the two approaches find the same
    // rows," not "does either one match a guess about the corpus."
    let fts5_conn = Connection::open(dir.path().join("full.sqlite3")).unwrap();

    let (avg_compact_us, avg_fts5_us) = run_queries(&compact, lines, &fts5_conn, queries);

    eprintln!("\n--- query correctness: compact-index(AND+verify) == FTS5(native phrase), all {} queries agree ---", queries.len());
    eprintln!("avg query latency — compact index (AND + verify): {avg_compact_us:.1}us");
    eprintln!("avg query latency — FTS5 detail=full (native phrase MATCH): {avg_fts5_us:.1}us");

    // --- Block-size sweep: how much size can be traded for the compact
    // index's speed advantage? block_size=1 (above) is the exact-row
    // baseline; each larger size groups that many consecutive rows into one
    // posting entry, shrinking the index at the cost of verifying more rows
    // per candidate (and, if a block straddles a query's true hit, more
    // false-positive blocks to rule out).
    eprintln!("\n--- block-size sweep (space vs. speed tradeoff), varint vs. stacked with Rice coding ---");
    eprintln!(
        "{:>10}  {:>12}  {:>10}  {:>12}  {:>12}  {:>14}  {:>14}",
        "block_size",
        "varint",
        "% of bs=1",
        "+rice",
        "rice % FTS5",
        "compact_us",
        "vs FTS5_full_us"
    );
    let mut baseline_bytes: Option<u64> = None;
    for &block_size in &[1u32, 4, 8, 16, 24, 32, 48, 64, 96, 128] {
        let compact_bs = build_compact_index(lines, block_size);
        let bytes = compact_bs.size_bytes() as u64;
        let rice_bytes_bs = compact_bs.rice_encoded_size_bytes() as u64;
        let baseline = *baseline_bytes.get_or_insert(bytes);
        let (avg_us, _) = run_queries(&compact_bs, lines, &fts5_conn, queries);
        eprintln!(
            "{:>10}  {:>12}  {:>9.1}%  {:>12}  {:>11.1}%  {:>12.1}us  {:>12.1}x",
            block_size,
            fmt_bytes(bytes),
            bytes as f64 / baseline as f64 * 100.0,
            fmt_bytes(rice_bytes_bs),
            rice_bytes_bs as f64 / fts5_full_index_bytes as f64 * 100.0,
            avg_us,
            avg_fts5_us / avg_us
        );
    }

    eprintln!("=== end {label} benchmark ===\n");
}

#[test]
#[ignore]
fn compact_index_vs_fts5_benchmark() {
    const N: usize = 300_000;
    let lines = generate_lines(N, 0x5EED_u64);

    // Inject known marker phrases at known rows so we can verify correctness
    // (not just size) against real FTS5 phrase-query behavior.
    let mut lines = lines;
    lines[1234] = "I am the one who knocks.".to_string();
    lines[98765] = "Say my name and let the spice flow.".to_string();
    lines[250_000] = "This is not breaking bad chemistry.".to_string();

    let queries = [
        "i am the one who knocks",
        "say my name",
        "spice flow",
        "breaking bad chemistry",
        "this phrase does not exist anywhere",
    ];
    run_full_benchmark("synthetic corpus", &lines, &queries);
}

/// Same methodology, run against a real slice of the user's own Plex
/// dialogue index instead of synthetic text — the synthetic generator places
/// terms uniformly at random, but real dialogue clusters by show/episode, so
/// this is the only way to know whether that structure changes the measured
/// ratios. Requires `real_sample.txt` at the repo root (one subtitle cue's
/// text per line, in original row order — see the project README/memory for
/// the export command); skipped with a clear message if it's not present,
/// since this file is real copyrighted content and deliberately untracked.
#[test]
#[ignore]
fn compact_index_vs_fts5_benchmark_real_data() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/real_sample.txt");
    let lines: Vec<String> = match std::fs::read_to_string(path) {
        Ok(contents) => contents.lines().map(|s| s.to_string()).collect(),
        Err(e) => {
            eprintln!("skipping real-data benchmark: couldn't read {path}: {e}");
            return;
        }
    };
    assert!(!lines.is_empty(), "{path} is empty");

    // Everyday phrases that are likely, but not guaranteed, to occur in a real
    // dialogue sample, plus one that almost certainly isn't —
    // correctness doesn't depend on these guesses being right either way,
    // since FTS5 is the oracle both sides are checked against.
    let queries = [
        "what are you doing here",
        "i think we should go",
        "wait for me outside",
        "stay with me for a little while",
        "this phrase almost certainly does not exist in this corpus",
    ];
    run_full_benchmark("real Plex dialogue sample", &lines, &queries);
}
