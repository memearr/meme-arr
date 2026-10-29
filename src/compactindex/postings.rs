//! Compressed posting lists and their streaming intersection.

use super::*;

/// A sorted list of row ids, delta-encoded as varints. Append-only: every id
/// added must be strictly greater than the last one already present.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Postings {
    pub(super) bytes: Vec<u8>,
    pub(super) last: u32,
    pub(super) len: usize,
}

impl Postings {
    pub fn append(&mut self, id: u32) {
        debug_assert!(
            self.len == 0 || id > self.last,
            "postings ids must be appended in strictly increasing order"
        );
        let delta = if self.len == 0 {
            id as u64
        } else {
            (id - self.last) as u64
        };
        encode_varint(delta, &mut self.bytes);
        self.last = id;
        self.len += 1;
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn last(&self) -> Option<u32> {
        if self.len == 0 {
            None
        } else {
            Some(self.last)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn size_bytes(&self) -> usize {
        self.bytes.len()
    }

    pub fn iter(&self) -> PostingsIter<'_> {
        PostingsIter {
            buf: &self.bytes,
            pos: 0,
            cur: 0,
            first: true,
        }
    }

    /// The raw deltas this list stores (the first "delta" is the first id
    /// itself, matching the varint encoding), without accumulating them into
    /// absolute ids. Used to re-encode this list under a different scheme
    /// for a size comparison.
    pub(super) fn raw_deltas(&self) -> Vec<u32> {
        let mut pos = 0;
        let mut out = Vec::with_capacity(self.len);
        while pos < self.bytes.len() {
            out.push(decode_varint(&self.bytes, &mut pos) as u32);
        }
        out
    }

    /// The byte size this list would occupy Golomb-Rice-coded instead of
    /// varint-coded, using a per-list parameter chosen from its own mean
    /// delta. `rice_encode` actually runs (this isn't a theoretical entropy
    /// estimate) so the byte count is real, measured output.
    pub fn rice_size_bytes(&self) -> usize {
        if self.len == 0 {
            return 0;
        }
        let deltas = self.raw_deltas();
        let mean = deltas.iter().map(|&d| d as f64).sum::<f64>() / deltas.len() as f64;
        let k = optimal_rice_k(mean);
        rice_encode(&deltas, k).len()
    }
}

pub struct PostingsIter<'a> {
    pub(super) buf: &'a [u8],
    pub(super) pos: usize,
    pub(super) cur: u32,
    pub(super) first: bool,
}

impl Iterator for PostingsIter<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        if self.pos >= self.buf.len() {
            return None;
        }
        let delta = decode_varint(self.buf, &mut self.pos) as u32;
        self.cur = if self.first { delta } else { self.cur + delta };
        self.first = false;
        Some(self.cur)
    }
}

/// Intersect several postings lists (AND query) via a k-way merge over their
/// decode iterators. Smallest list first keeps the common case (one rare
/// term quickly narrowing things down) cheap.
pub fn intersect(lists: &[&Postings]) -> Vec<u32> {
    if lists.is_empty() || lists.iter().any(|l| l.is_empty()) {
        return Vec::new();
    }
    let mut order: Vec<&Postings> = lists.to_vec();
    order.sort_by_key(|l| l.len());

    let mut iters: Vec<std::iter::Peekable<PostingsIter>> =
        order.iter().map(|l| l.iter().peekable()).collect();
    let mut out = Vec::new();

    'outer: while let Some(&candidate) = iters[0].peek() {
        for it in iters.iter_mut().skip(1) {
            loop {
                match it.peek() {
                    Some(&v) if v < candidate => {
                        it.next();
                    }
                    Some(&v) if v == candidate => break,
                    _ => {
                        // Either exhausted or past the candidate: candidate
                        // can't be in every list, advance the first list.
                        iters[0].next();
                        continue 'outer;
                    }
                }
            }
        }
        out.push(candidate);
        for it in iters.iter_mut() {
            it.next();
        }
    }
    out
}
