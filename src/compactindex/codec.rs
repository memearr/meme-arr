//! Byte and bit-level encoding: varints, bit I/O, and Rice coding of posting deltas.

use super::*;

/// Encode `v` as a LEB128 varint, appended to `out`.
pub(super) fn encode_varint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

/// Decode a LEB128 varint starting at `pos`, advancing `pos` past it.
pub(super) fn decode_varint(buf: &[u8], pos: &mut usize) -> u64 {
    let mut result = 0u64;
    let mut shift = 0;
    loop {
        let byte = buf[*pos];
        *pos += 1;
        result |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    result
}

/// On-disk format for [`CompactIndex::to_bytes`]/[`CompactIndex::from_bytes`].
pub(super) const COMPACT_INDEX_MAGIC: &[u8; 4] = b"PMCI";

/// Bump this whenever the byte layout changes. [`CompactIndex::from_bytes`]
/// rejects anything else outright rather than guess — a stale saved index is
/// always cheap to rebuild from `subtitle_cue`, so there's no reason to risk
/// silently misreading one.
pub(super) const COMPACT_INDEX_FORMAT_VERSION: u32 = 1;

/// The `block_size` chosen for production use, picked off a real sweep
/// against the user's own library (`tests/compact_index_benchmark.rs`,
/// `compact_index_vs_fts5_benchmark_real_data`): 2.03 MB — 9.6% of FTS5
/// detail=full's size — while still running ~3.7x faster per query than
/// FTS5. Not a hardcoded default on `CompactIndex` itself (that stays 1, for
/// every existing exact-row test and use site) — callers building the real,
/// persisted production index should pass this explicitly.
pub const PRODUCTION_BLOCK_SIZE: u32 = 32;

/// Read a little-endian `u32` at `pos`, advancing past it, or fail cleanly
/// on a truncated buffer instead of panicking.
pub(super) fn read_u32(buf: &[u8], pos: &mut usize) -> Result<u32> {
    ensure!(*pos + 4 <= buf.len(), "truncated compact index file");
    let v = u32::from_le_bytes(buf[*pos..*pos + 4].try_into().unwrap());
    *pos += 4;
    Ok(v)
}

/// A bit-level writer, MSB-first within each byte, for Golomb-Rice coding.
/// LEB128 varints round every value up to a whole byte; small deltas (the
/// common case for a frequent term) waste up to 7 bits of padding each time.
/// Rice coding spends exactly as many bits as the chosen parameter needs.
pub(super) struct BitWriter {
    pub(super) bytes: Vec<u8>,
    pub(super) cur: u8,
    pub(super) nbits: u8,
}

impl BitWriter {
    pub(super) fn new() -> Self {
        Self {
            bytes: Vec::new(),
            cur: 0,
            nbits: 0,
        }
    }

    pub(super) fn write_bit(&mut self, bit: bool) {
        self.cur = (self.cur << 1) | (bit as u8);
        self.nbits += 1;
        if self.nbits == 8 {
            self.bytes.push(self.cur);
            self.cur = 0;
            self.nbits = 0;
        }
    }

    pub(super) fn write_bits(&mut self, value: u32, nbits: u32) {
        for i in (0..nbits).rev() {
            self.write_bit((value >> i) & 1 == 1);
        }
    }

    /// `q` 1-bits followed by a terminating 0-bit.
    pub(super) fn write_unary(&mut self, q: u32) {
        for _ in 0..q {
            self.write_bit(true);
        }
        self.write_bit(false);
    }

    /// Pad the final partial byte with zero bits and return the buffer.
    pub(super) fn finish(mut self) -> Vec<u8> {
        if self.nbits > 0 {
            self.cur <<= 8 - self.nbits;
            self.bytes.push(self.cur);
        }
        self.bytes
    }
}

// `BitReader`/`rice_decode` exist to prove `rice_encode`'s output actually
// round-trips (see the `rice_roundtrips_*` tests below) — nothing outside
// tests decodes Rice-coded postings yet, since this encoding isn't wired
// into the live query path, only compared for size via `rice_size_bytes`.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct BitReader<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) pos: usize,
}

#[cfg_attr(not(test), allow(dead_code))]
impl<'a> BitReader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    pub(super) fn read_bit(&mut self) -> bool {
        let byte = self.bytes[self.pos / 8];
        let bit = (byte >> (7 - (self.pos % 8))) & 1;
        self.pos += 1;
        bit == 1
    }

    pub(super) fn read_bits(&mut self, nbits: u32) -> u32 {
        let mut v = 0u32;
        for _ in 0..nbits {
            v = (v << 1) | self.read_bit() as u32;
        }
        v
    }

    pub(super) fn read_unary(&mut self) -> u32 {
        let mut q = 0;
        while self.read_bit() {
            q += 1;
        }
        q
    }
}

/// The Rice parameter that minimizes expected code length for a geometric
/// delta distribution with mean `mean` (Gallager & Van Voorhis'
/// approximation: pick `k` so `2^k` is close to `0.69 * mean`). This is a
/// closed-form estimate, not a search over all `k` — good enough to pick a
/// sane parameter per posting list without an expensive trial-encode loop.
pub(super) fn optimal_rice_k(mean: f64) -> u32 {
    let biased = mean * std::f64::consts::LN_2;
    if biased < 1.0 {
        0
    } else {
        biased.log2().round() as u32
    }
}

/// Golomb-Rice encode `deltas` with parameter `k`: each value splits into a
/// quotient (`delta >> k`, unary-coded) and a `k`-bit remainder.
pub(super) fn rice_encode(deltas: &[u32], k: u32) -> Vec<u8> {
    let mut w = BitWriter::new();
    for &d in deltas {
        w.write_unary(d >> k);
        if k > 0 {
            w.write_bits(d & ((1 << k) - 1), k);
        }
    }
    w.finish()
}

/// Inverse of [`rice_encode`]: decode exactly `count` values.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn rice_decode(bytes: &[u8], k: u32, count: usize) -> Vec<u32> {
    let mut r = BitReader::new(bytes);
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let q = r.read_unary();
        let rem = if k > 0 { r.read_bits(k) } else { 0 };
        out.push((q << k) | rem);
    }
    out
}
