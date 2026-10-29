//! A purpose-built, compressed inverted index for exact-phrase dialogue
//! search over a large, append-mostly corpus of short text lines.
//!
//! # Why this exists
//!
//! SQLite FTS5's default mode (`detail=full`) stores, for every token
//! occurrence, its exact position within its row — that's what lets it
//! answer phrase queries natively. For a corpus of tens of millions of short
//! subtitle lines, that per-occurrence position data is the dominant cost:
//! common words occur constantly, so their position lists are enormous.
//!
//! We don't actually need the index to *prove* phrase-adjacency. We already
//! store the authoritative, untouched line text (`subtitle_cue.text`) for
//! burning into video — so the index only has to answer "which rows contain
//! *all* of these terms" cheaply and compactly; a caller can then verify the
//! exact phrase against the real text for the (small) candidate set this
//! returns. This is the classic information-retrieval pattern of candidate
//! generation + verification (the same idea behind a Bloom filter, or
//! `grep -F` prefiltering before a slower regex engine runs) — trading a
//! cheap, infrequent verification step for a structurally smaller index.
//!
//! # Design
//!
//! - A term dictionary: word → dense `u32` term id (a `HashMap`).
//! - Per term, a **sorted, delta-encoded** list of row ids that contain it,
//!   stored as a byte blob of LEB128 varints. Row ids only grow over time (a
//!   newly-indexed cue always gets a larger id than anything already
//!   indexed), so appending new postings is just "encode the new deltas and
//!   append the bytes" — no re-sort, no rewrite of existing data.
//! - A query ANDs several terms by streaming-merging their postings lists:
//!   decode-and-intersect together rather than materializing each list in
//!   full first, so an early mismatch skips ahead cheaply.
//! - Common stopwords ARE indexed (dropping them entirely would leave no
//!   correct way to answer a query made only of stopwords), but they're
//!   excluded from the hot-path intersection — their postings are large and
//!   rarely help narrow results — falling back to the full term set only
//!   when every term in a query is a stopword, so correctness never breaks,
//!   it only degrades to "slower for that one rare query shape."
//!
//! This module is a **prototype**, benchmarked against real SQLite FTS5
//! tables in `tests::compact_index_vs_fts5_benchmark` — it is not yet wired
//! into the live search path.
//!
//! # Persistence
//!
//! `to_bytes`/`from_bytes` (and the file-backed `save_to`/`load_from`) give
//! this a real on-disk form, but it's treated as a **rebuildable derived
//! cache**, never authoritative storage — `subtitle_cue`/`subtitle_fts`
//! stay the source of truth. The reason: `Postings::append` requires
//! strictly increasing ids forever, but production really does delete rows
//! (media removal/remap, `indexer_version` bumps forcing reprocessing), and
//! an append-only byte-packed posting list can't patch a delete out of its
//! middle. So there's no in-place update here at all — `rebuild_from_db`
//! does a full rebuild, and the intended usage is: on any change, rebuild in
//! the background, then atomically swap in the new file (`save_to` already
//! writes via temp-file-then-rename so a reader never sees a partial
//! write). A `COMPACT_INDEX_FORMAT_VERSION` mismatch on load means "rebuild
//! instead of reading this," not "crash."

use anyhow::{ensure, Context, Result};
use std::collections::HashMap;

mod codec;
mod postings;
#[cfg(test)]
mod tests;

pub use self::codec::*;
pub use self::postings::*;

/// A compact set of common English stopwords. These are never indexed —
/// they're exactly the terms whose posting lists `detail=full` spends the
/// most space on (they occur constantly), and they carry the least value as
/// a candidate-narrowing filter.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "been", "but", "by", "do", "for", "from", "had",
    "has", "have", "he", "her", "his", "i", "in", "is", "it", "its", "me", "my", "no", "not", "of",
    "on", "or", "our", "she", "so", "that", "the", "their", "there", "they", "this", "to", "was",
    "we", "were", "will", "with", "would", "you", "your",
];

fn is_stopword(term: &str) -> bool {
    STOPWORDS.binary_search(&term).is_ok()
}

/// Tokenize into lowercase word tokens (alphanumeric runs), mirroring the
/// word-boundary splitting FTS5's `unicode61` tokenizer does closely enough
/// for an apples-to-apples size/behavior comparison, without pulling in a
/// full Unicode segmentation dependency for this prototype.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

#[derive(Debug, PartialEq)]
pub struct CompactIndex {
    term_to_id: HashMap<String, u32>,
    postings: Vec<Postings>,
    /// Rows per posting entry. `1` (the default) means a posting names an
    /// exact row — identical behavior to a plain per-row index. Anything
    /// larger groups `block_size` consecutive row ids into one posting,
    /// trading candidate precision for index size: see `block_range`.
    block_size: u32,
    /// One past the largest row id ever indexed (0 if none yet) — purely
    /// informational metadata (e.g. for a caller deciding whether a saved
    /// index is stale against the live row count), not used by queries.
    row_count: u32,
}

impl Default for CompactIndex {
    fn default() -> Self {
        Self {
            term_to_id: HashMap::new(),
            postings: Vec::new(),
            block_size: 1,
            row_count: 0,
        }
    }
}

impl CompactIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Group rows into blocks of `block_size` for indexing — a smaller,
    /// coarser index (fewer, larger-granularity postings) at the cost of the
    /// caller needing to scan and verify every row in a surviving candidate
    /// block, not just one exact row. `block_size` of 1 is exact (the
    /// default); see module docs for the space/speed tradeoff this trades on.
    pub fn with_block_size(block_size: u32) -> Self {
        Self {
            block_size: block_size.max(1),
            ..Self::default()
        }
    }

    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    /// One past the largest row id ever indexed (0 if nothing's been indexed
    /// yet).
    pub fn row_count(&self) -> u32 {
        self.row_count
    }

    /// The `[start, end)` row-id range a block id covers — expand a
    /// candidate block to the rows the caller actually needs to check.
    pub fn block_range(&self, block_id: u32) -> std::ops::Range<u32> {
        let start = block_id * self.block_size;
        start..(start + self.block_size)
    }

    fn term_id(&mut self, term: &str) -> u32 {
        if let Some(&id) = self.term_to_id.get(term) {
            return id;
        }
        let id = self.postings.len() as u32;
        self.term_to_id.insert(term.to_string(), id);
        self.postings.push(Postings::default());
        id
    }

    /// Index one line. `row_id`s must arrive in non-decreasing order across
    /// calls (true for a batch build over an append-ordered table).
    ///
    /// Stopwords ARE indexed here (unlike a "never store them" design) —
    /// they're just excluded from the hot path at query time below. We still
    /// need their postings available for the rare case a query is made
    /// entirely of stopwords; the alternative (truly never indexing them)
    /// leaves no correct way to answer that query at all short of a full
    /// table scan, which defeats the point of having an index.
    pub fn index_line(&mut self, row_id: u32, text: &str) {
        self.row_count = self.row_count.max(row_id + 1);
        let block_id = row_id / self.block_size;
        let mut seen = std::collections::HashSet::new();
        for tok in tokenize(text) {
            if tok.len() < 2 {
                continue;
            }
            if !seen.insert(tok.clone()) {
                continue; // one posting per block per term, not per occurrence
            }
            let id = self.term_id(&tok);
            let p = &mut self.postings[id as usize];
            // Skip re-appending the same block: common when block_size > 1
            // and several consecutive rows land in it, and also what makes
            // block_size == 1 degrade to exactly "one posting per row."
            if p.last() != Some(block_id) {
                p.append(block_id);
            }
        }
    }

    /// Block ids whose row range might contain a line matching ALL of
    /// `terms` (a superset of true phrase matches — the caller must still
    /// scan each candidate block's rows via `block_range` and verify against
    /// the real stored text; at `block_size() == 1` a block IS a row, so this
    /// degenerates to exact per-row candidates with no extra scanning).
    /// Stopwords are excluded from the intersection (their postings are
    /// large and rarely help narrow results), falling back to the full term
    /// list only if every term given is a stopword — otherwise a query like
    /// "to be or not to be" would wrongly find nothing.
    pub fn query_candidate_blocks(&self, terms: &[&str]) -> Vec<u32> {
        let meaningful: Vec<&str> = terms.iter().copied().filter(|t| !is_stopword(t)).collect();
        let use_terms: &[&str] = if meaningful.is_empty() {
            terms
        } else {
            &meaningful
        };

        let mut lists = Vec::with_capacity(use_terms.len());
        for t in use_terms {
            match self.term_to_id.get(&t.to_lowercase()) {
                Some(&id) => lists.push(&self.postings[id as usize]),
                None => return Vec::new(), // a required term isn't indexed anywhere
            }
        }
        intersect(&lists)
    }

    /// Total size of the dictionary + all postings, for benchmarking against
    /// an equivalent FTS5 table's on-disk size.
    pub fn size_bytes(&self) -> usize {
        let dict_bytes: usize = self.term_to_id.keys().map(|k| k.len() + 4).sum();
        let postings_bytes: usize = self.postings.iter().map(|p| p.size_bytes()).sum();
        dict_bytes + postings_bytes
    }

    pub fn term_count(&self) -> usize {
        self.term_to_id.len()
    }

    /// What `size_bytes()` would be if every posting list were Golomb-Rice
    /// coded instead of varint-coded (one extra byte per list to store its
    /// chosen `k`, the way a real on-disk format would need to). A real,
    /// measured alternative encoding of the same data — not a theoretical
    /// entropy estimate — for comparing against the varint baseline.
    pub fn rice_encoded_size_bytes(&self) -> usize {
        let dict_bytes: usize = self.term_to_id.keys().map(|k| k.len() + 4).sum();
        let postings_bytes: usize = self.postings.iter().map(|p| 1 + p.rice_size_bytes()).sum();
        dict_bytes + postings_bytes
    }

    /// Serialize to a single, self-contained byte buffer: a header, the term
    /// dictionary (in term-id order), then each term's postings (same
    /// order). Varint-coded, matching what `Postings`/`PostingsIter` can
    /// actually read back today — see [`CompactIndex::load_from`] for the
    /// on-disk round trip this backs.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(COMPACT_INDEX_MAGIC);
        out.extend_from_slice(&COMPACT_INDEX_FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&self.block_size.to_le_bytes());
        out.extend_from_slice(&self.row_count.to_le_bytes());
        out.extend_from_slice(&(self.postings.len() as u32).to_le_bytes());

        // Term dictionary, placed in term-id order so the postings section
        // below (also written in that order) lines up with it positionally
        // on load — no need to store ids explicitly.
        let mut ordered_terms: Vec<&str> = vec![""; self.postings.len()];
        for (term, &id) in &self.term_to_id {
            ordered_terms[id as usize] = term;
        }
        for term in &ordered_terms {
            out.extend_from_slice(&(term.len() as u32).to_le_bytes());
            out.extend_from_slice(term.as_bytes());
        }

        for p in &self.postings {
            out.extend_from_slice(&p.last.to_le_bytes());
            out.extend_from_slice(&(p.len as u32).to_le_bytes());
            out.extend_from_slice(&(p.bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(&p.bytes);
        }
        out
    }

    /// Inverse of [`to_bytes`](Self::to_bytes). Fails on a bad magic number,
    /// a truncated buffer, or a format version this build doesn't
    /// understand — the last case is deliberate: a caller that hits it
    /// should treat the saved file as stale and rebuild via
    /// [`rebuild_from_db`](Self::rebuild_from_db) rather than try to read it.
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        let mut pos = 0usize;
        ensure!(
            buf.len() >= COMPACT_INDEX_MAGIC.len()
                && &buf[..COMPACT_INDEX_MAGIC.len()] == COMPACT_INDEX_MAGIC,
            "not a compact index file (bad magic)"
        );
        pos += COMPACT_INDEX_MAGIC.len();

        let format_version = read_u32(buf, &mut pos)?;
        ensure!(
            format_version == COMPACT_INDEX_FORMAT_VERSION,
            "unsupported compact index format version {format_version} \
             (this build understands {COMPACT_INDEX_FORMAT_VERSION}) — rebuild instead of loading"
        );
        let block_size = read_u32(buf, &mut pos)?.max(1);
        let row_count = read_u32(buf, &mut pos)?;
        let term_count = read_u32(buf, &mut pos)? as usize;

        let mut term_to_id = HashMap::with_capacity(term_count);
        for id in 0..term_count {
            let len = read_u32(buf, &mut pos)? as usize;
            ensure!(
                pos + len <= buf.len(),
                "truncated compact index file (term dictionary)"
            );
            let term = std::str::from_utf8(&buf[pos..pos + len])
                .context("invalid utf8 in compact index term dictionary")?
                .to_string();
            pos += len;
            term_to_id.insert(term, id as u32);
        }

        let mut postings = Vec::with_capacity(term_count);
        for _ in 0..term_count {
            let last = read_u32(buf, &mut pos)?;
            let len = read_u32(buf, &mut pos)? as usize;
            let blob_len = read_u32(buf, &mut pos)? as usize;
            ensure!(
                pos + blob_len <= buf.len(),
                "truncated compact index file (postings)"
            );
            let bytes = buf[pos..pos + blob_len].to_vec();
            pos += blob_len;
            postings.push(Postings { bytes, last, len });
        }

        Ok(Self {
            term_to_id,
            postings,
            block_size,
            row_count,
        })
    }

    /// Write this index to `path` atomically: the full buffer is written to
    /// a temp file alongside `path`, fsync'd, then renamed into place.
    /// Renaming (same filesystem) is atomic, so a reader can never observe a
    /// partially-written file, and a crash mid-write leaves the previous
    /// `path` (if any) untouched.
    pub fn save_to(&self, path: &std::path::Path) -> Result<()> {
        let tmp_path = path.with_extension("tmp");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp_path)
                .with_context(|| format!("creating {}", tmp_path.display()))?;
            f.write_all(&self.to_bytes())
                .with_context(|| format!("writing {}", tmp_path.display()))?;
            f.sync_all()
                .with_context(|| format!("fsyncing {}", tmp_path.display()))?;
        }
        std::fs::rename(&tmp_path, path)
            .with_context(|| format!("renaming {} -> {}", tmp_path.display(), path.display()))?;
        Ok(())
    }

    /// Read a file written by [`save_to`](Self::save_to).
    pub fn load_from(path: &std::path::Path) -> Result<Self> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_bytes(&bytes)
    }

    /// Build a fresh index from the live `subtitle_cue` table, in row order.
    /// This is the index's only real "update" path — see the module docs on
    /// why it's a full rebuild rather than an in-place patch (deletes don't
    /// fit the append-only postings representation).
    pub fn rebuild_from_db(conn: &rusqlite::Connection, block_size: u32) -> Result<Self> {
        let mut idx = Self::with_block_size(block_size);
        let mut stmt = conn.prepare("SELECT id, text FROM subtitle_cue ORDER BY id")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let text: String = row.get(1)?;
            idx.index_line(id as u32, &text);
        }
        Ok(idx)
    }
}

/// Verify an exact phrase occurs (as consecutive tokens, case-insensitive)
/// within `text` — the step that recovers exact-phrase correctness after the
/// index itself only proved "contains all these terms, in some order."
pub fn phrase_matches(text: &str, phrase_terms: &[String]) -> bool {
    if phrase_terms.is_empty() {
        return false;
    }
    let tokens = tokenize(text);
    tokens
        .windows(phrase_terms.len())
        .any(|w| w == phrase_terms)
}
