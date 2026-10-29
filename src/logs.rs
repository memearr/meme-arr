//! In-app log viewer (System -> Logs), like Sonarr's Log Files page: a ring
//! buffer of recent `tracing` output you can read in the browser instead of
//! `docker logs`, and a log level you can change without a restart.
//!
//! Everything the process logs also still goes to stdout as before. The
//! buffer holds only the most recent `CAPACITY` entries, in memory -- it's a
//! convenience tail, not an archive (and gone on restart; the Activity log is
//! the persistent, curated record).
//!
//! Messages are scrubbed of secrets (Plex tokens, bearer tokens, `token=` /
//! `password=` query values) before they're stored, since error text can
//! carry full request URLs.

use crate::sync::MutexExt;
use serde::Serialize;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{reload, EnvFilter, Layer, Registry};

const CAPACITY: usize = 2000;
/// Cap on a single stored message, so one huge error can't eat the buffer.
const MAX_MESSAGE: usize = 4000;

pub const LEVELS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    /// Increases by one per entry; lets a poller ask for "everything after".
    pub id: u64,
    pub ts_ms: i64,
    pub level: &'static str,
    pub target: String,
    pub message: String,
}

#[derive(Default)]
struct Buffer {
    entries: VecDeque<Entry>,
    next_id: u64,
}

pub struct LogControl {
    buffer: Mutex<Buffer>,
    handle: reload::Handle<EnvFilter, Registry>,
    /// The level the app crate is currently logging at.
    level: Mutex<String>,
}

/// Install the global subscriber: stdout (as before), the in-memory buffer,
/// and a reloadable filter. Honors `RUST_LOG` for the starting filter.
pub fn init() -> Arc<LogControl> {
    let from_env = EnvFilter::try_from_default_env();
    let level = if from_env.is_ok() {
        "env".to_string()
    } else {
        "info".to_string()
    };
    let filter = from_env.unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter_layer, handle) = reload::Layer::new(filter);
    let control = Arc::new(LogControl {
        buffer: Mutex::new(Buffer::default()),
        handle,
        level: Mutex::new(level),
    });
    Registry::default()
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer())
        .with(BufferLayer(control.clone()))
        .init();
    control
}

impl LogControl {
    /// A control not attached to any subscriber, for tests.
    #[cfg(test)]
    fn detached() -> Arc<LogControl> {
        let (_layer, handle) = reload::Layer::new(EnvFilter::new("info"));
        Arc::new(LogControl {
            buffer: Mutex::new(Buffer::default()),
            handle,
            level: Mutex::new("info".into()),
        })
    }

    pub fn level(&self) -> String {
        self.level.locked().clone()
    }

    /// Change the app's log level at runtime. Dependencies stay at `info`
    /// (hyper/reqwest at debug would drown everything), except for
    /// error/warn, which apply globally.
    pub fn set_level(&self, level: &str) -> Result<(), String> {
        let directive = directive_for(level)?;
        self.handle
            .reload(EnvFilter::new(directive))
            .map_err(|e| format!("couldn't apply log level: {e}"))?;
        *self.level.locked() = level.to_string();
        Ok(())
    }

    fn push(&self, level: &'static str, target: &str, message: String) {
        let mut b = self.buffer.locked();
        let id = b.next_id;
        b.next_id += 1;
        if b.entries.len() == CAPACITY {
            b.entries.pop_front();
        }
        b.entries.push_back(Entry {
            id,
            ts_ms: now_ms(),
            level,
            target: target.to_string(),
            message,
        });
    }

    /// Up to `limit` newest entries at or above `min_level` whose text
    /// contains `contains` (case-insensitive), oldest first, only those with
    /// `id > after` when given (for incremental polling).
    pub fn query(
        &self,
        min_level: &str,
        contains: &str,
        after: Option<u64>,
        limit: usize,
    ) -> Vec<Entry> {
        let min = rank(min_level);
        let needle = contains.trim().to_lowercase();
        let b = self.buffer.locked();
        let mut out: Vec<Entry> = b
            .entries
            .iter()
            .rev()
            .filter(|e| after.map_or(true, |a| e.id > a))
            .filter(|e| rank(e.level) <= min)
            .filter(|e| {
                needle.is_empty()
                    || e.message.to_lowercase().contains(&needle)
                    || e.target.to_lowercase().contains(&needle)
            })
            .take(limit)
            .cloned()
            .collect();
        out.reverse();
        out
    }
}

/// Lower is more severe: error 0 .. trace 4. Unknown levels show everything.
fn rank(level: &str) -> usize {
    LEVELS
        .iter()
        .position(|l| *l == level)
        .unwrap_or(LEVELS.len() - 1)
}

fn directive_for(level: &str) -> Result<String, String> {
    match level {
        "error" | "warn" | "info" => Ok(level.to_string()),
        "debug" | "trace" => Ok(format!("info,meme_arr={level}")),
        other => Err(format!(
            "unknown log level '{other}' (use one of: {})",
            LEVELS.join(", ")
        )),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

struct BufferLayer(Arc<LogControl>);

impl<S: Subscriber> Layer<S> for BufferLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut v = MessageVisitor::default();
        event.record(&mut v);
        let mut message = scrub(&v.finish());
        if message.len() > MAX_MESSAGE {
            message.truncate(MAX_MESSAGE);
            message.push('…');
        }
        let level = match *event.metadata().level() {
            Level::ERROR => "error",
            Level::WARN => "warn",
            Level::INFO => "info",
            Level::DEBUG => "debug",
            Level::TRACE => "trace",
        };
        self.0.push(level, event.metadata().target(), message);
    }
}

/// The event's `message` field, with any other fields appended `k=v`.
#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: String,
}

impl MessageVisitor {
    fn finish(self) -> String {
        if self.fields.is_empty() {
            self.message
        } else {
            format!("{} {}", self.message, self.fields.trim_start())
        }
    }
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
}

/// Mask values that look like credentials: `X-Plex-Token=…`, `token=…`,
/// `password=…`, `secret=…`, `api_key=…` (up to the next `&`, quote, space or
/// bracket), and `Bearer …`.
pub fn scrub(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let lower = s.to_lowercase();
    let bytes = s.as_bytes();
    let keys = [
        "token=",
        "password=",
        "secret=",
        "api_key=",
        "apikey=",
        "bearer ",
    ];
    let mut i = 0;
    'outer: while i < bytes.len() {
        for k in keys {
            if lower[i..].starts_with(k) {
                out.push_str(&s[i..i + k.len()]);
                let mut j = i + k.len();
                while j < bytes.len()
                    && !matches!(
                        bytes[j],
                        b'&' | b'"' | b'\'' | b' ' | b')' | b']' | b'\n' | b','
                    )
                {
                    j += 1;
                }
                out.push_str("***");
                i = j;
                continue 'outer;
            }
        }
        // `s` is valid UTF-8 and we only ever advance by whole chars here.
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_masks_credentials_but_leaves_the_rest() {
        assert_eq!(
            scrub("error sending request for url (http://plex:32400/library/sections?X-Plex-Token=abc123&x=1)"),
            "error sending request for url (http://plex:32400/library/sections?X-Plex-Token=***&x=1)"
        );
        assert_eq!(
            scrub("Authorization: Bearer eyJhbGci.x.y done"),
            "Authorization: Bearer *** done"
        );
        assert_eq!(
            scrub("login password=hunter2, ok"),
            "login password=***, ok"
        );
        assert_eq!(scrub("API_KEY=memearr_abc"), "API_KEY=***");
        assert_eq!(
            scrub("nothing secret here — café ✓"),
            "nothing secret here — café ✓"
        );
    }

    #[test]
    fn level_ranking_and_directives() {
        assert!(rank("error") < rank("warn") && rank("warn") < rank("info"));
        assert_eq!(directive_for("debug").unwrap(), "info,meme_arr=debug");
        assert_eq!(directive_for("warn").unwrap(), "warn");
        assert!(directive_for("loud").is_err());
    }

    #[test]
    fn buffer_keeps_only_the_newest_and_assigns_increasing_ids() {
        let c = LogControl::detached();
        for i in 0..(CAPACITY + 5) {
            c.push("info", "t", format!("m{i}"));
        }
        let all = c.query("trace", "", None, CAPACITY + 10);
        assert_eq!(all.len(), CAPACITY);
        assert_eq!(all[0].message, "m5");
        assert_eq!(all.last().unwrap().id, (CAPACITY + 4) as u64);
    }

    #[test]
    fn query_filters_by_level_text_and_after_id_and_limits_to_newest() {
        let c = LogControl::detached();
        c.push("debug", "app", "noisy detail".into());
        c.push("info", "app", "indexed 12 items".into());
        c.push("warn", "app::index", "Subtitle extract FAILED".into());
        c.push("error", "app", "boom".into());

        let msgs = |v: Vec<Entry>| v.into_iter().map(|e| e.message).collect::<Vec<_>>();
        assert_eq!(
            msgs(c.query("warn", "", None, 10)),
            ["Subtitle extract FAILED", "boom"]
        );
        assert_eq!(
            msgs(c.query("trace", "failed", None, 10)),
            ["Subtitle extract FAILED"]
        );
        assert_eq!(
            msgs(c.query("trace", "app::index", None, 10)),
            ["Subtitle extract FAILED"]
        );
        assert_eq!(
            msgs(c.query("trace", "", Some(1), 10)),
            ["Subtitle extract FAILED", "boom"]
        );
        // limit keeps the newest, still oldest-first.
        assert_eq!(
            msgs(c.query("trace", "", None, 2)),
            ["Subtitle extract FAILED", "boom"]
        );
    }

    #[test]
    fn events_are_captured_with_fields_and_scrubbed() {
        let c = LogControl::detached();
        let sub = Registry::default().with(BufferLayer(c.clone()));
        tracing::subscriber::with_default(sub, || {
            tracing::warn!(attempt = 3, "Plex failed: http://x/?X-Plex-Token=SECRET");
        });
        let e = &c.query("trace", "", None, 5)[0];
        assert_eq!(e.level, "warn");
        assert!(e.message.contains("Plex failed"));
        assert!(e.message.contains("attempt=3"));
        assert!(!e.message.contains("SECRET"), "{}", e.message);
    }
}
