//! Per-IP lockout for the password login endpoint, with exponential backoff.
//!
//! In-memory only: a restart clears it, and it isn't shared across replicas —
//! this app only ever runs as a single instance, so neither matters.
//!
//! Keyed on the TCP peer address, never `X-Forwarded-For`: that header is
//! client-controlled, so trusting it would let an attacker claim a fresh "IP"
//! on every attempt and bypass the limit entirely. Behind a reverse proxy,
//! every proxied request shares the proxy's bucket — stricter than ideal, but
//! fail-safe, and fine for a single-admin app.

use crate::sync::MutexExt;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Failed attempts allowed before the first lockout.
const MAX_ATTEMPTS: u32 = 5;
/// First lockout length; doubles with each further failure.
const BASE_LOCKOUT: Duration = Duration::from_secs(30);
const MAX_LOCKOUT: Duration = Duration::from_secs(15 * 60);
/// Entries idle longer than this are dropped, so a long uptime can't grow
/// the map without bound.
const ENTRY_TTL: Duration = Duration::from_secs(60 * 60);

struct Attempts {
    failures: u32,
    last_seen: Instant,
    locked_until: Option<Instant>,
}

#[derive(Default)]
pub struct LoginLimiter {
    state: Mutex<HashMap<IpAddr, Attempts>>,
}

impl LoginLimiter {
    /// `Some(retry_after)` if `ip` is locked out right now.
    pub fn check(&self, ip: IpAddr) -> Option<Duration> {
        self.check_at(ip, Instant::now())
    }

    /// Record a failed attempt. `Some(duration)` if it just locked `ip` out
    /// (or extended the lockout) -- so the caller can log it.
    pub fn record_failure(&self, ip: IpAddr) -> Option<Duration> {
        self.record_failure_at(ip, Instant::now())
    }

    pub fn record_success(&self, ip: IpAddr) {
        self.state.locked().remove(&ip);
    }

    fn check_at(&self, ip: IpAddr, now: Instant) -> Option<Duration> {
        let mut map = self.state.locked();
        map.retain(|_, a| now.saturating_duration_since(a.last_seen) < ENTRY_TTL);
        map.get(&ip)
            .and_then(|a| a.locked_until)
            .filter(|&until| until > now)
            .map(|until| until - now)
    }

    fn record_failure_at(&self, ip: IpAddr, now: Instant) -> Option<Duration> {
        let mut map = self.state.locked();
        let a = map.entry(ip).or_insert(Attempts {
            failures: 0,
            last_seen: now,
            locked_until: None,
        });
        a.failures += 1;
        a.last_seen = now;
        if a.failures >= MAX_ATTEMPTS {
            let lock = lockout_for(a.failures);
            a.locked_until = Some(now + lock);
            return Some(lock);
        }
        None
    }
}

/// 30s at the 5th failure, doubling each failure after, capped at 15 min.
fn lockout_for(failures: u32) -> Duration {
    // Capping the shift keeps it far from overflow; MAX_LOCKOUT caps the
    // result long before the shift limit is reached anyway.
    let doublings = failures.saturating_sub(MAX_ATTEMPTS).min(16);
    BASE_LOCKOUT
        .saturating_mul(1u32 << doublings)
        .min(MAX_LOCKOUT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    const IP: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 0, 50));
    const OTHER: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 0, 51));

    #[test]
    fn allows_attempts_under_the_threshold() {
        let l = LoginLimiter::default();
        let now = Instant::now();
        for _ in 0..MAX_ATTEMPTS - 1 {
            l.record_failure_at(IP, now);
        }
        assert_eq!(l.check_at(IP, now), None);
    }

    #[test]
    fn locks_out_at_the_threshold_and_only_that_ip() {
        let l = LoginLimiter::default();
        let now = Instant::now();
        for _ in 0..MAX_ATTEMPTS {
            l.record_failure_at(IP, now);
        }
        assert_eq!(l.check_at(IP, now), Some(BASE_LOCKOUT));
        assert_eq!(l.check_at(OTHER, now), None);
    }

    #[test]
    fn record_failure_reports_the_lockout_only_once_it_starts() {
        let l = LoginLimiter::default();
        let now = Instant::now();
        for _ in 0..MAX_ATTEMPTS - 1 {
            assert_eq!(l.record_failure_at(IP, now), None);
        }
        assert_eq!(l.record_failure_at(IP, now), Some(BASE_LOCKOUT));
    }

    #[test]
    fn lockout_expires() {
        let l = LoginLimiter::default();
        let now = Instant::now();
        for _ in 0..MAX_ATTEMPTS {
            l.record_failure_at(IP, now);
        }
        assert_eq!(l.check_at(IP, now + BASE_LOCKOUT), None);
    }

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(lockout_for(MAX_ATTEMPTS), BASE_LOCKOUT);
        assert_eq!(lockout_for(MAX_ATTEMPTS + 1), BASE_LOCKOUT * 2);
        assert_eq!(lockout_for(MAX_ATTEMPTS + 2), BASE_LOCKOUT * 4);
        assert_eq!(lockout_for(MAX_ATTEMPTS + 50), MAX_LOCKOUT);
    }

    #[test]
    fn success_clears_the_counter() {
        let l = LoginLimiter::default();
        let now = Instant::now();
        for _ in 0..MAX_ATTEMPTS - 1 {
            l.record_failure_at(IP, now);
        }
        l.record_success(IP);
        l.record_failure_at(IP, now);
        assert_eq!(l.check_at(IP, now), None);
    }

    #[test]
    fn idle_entries_are_swept() {
        let l = LoginLimiter::default();
        let now = Instant::now();
        l.record_failure_at(IP, now);
        l.check_at(OTHER, now + ENTRY_TTL);
        assert!(l.state.locked().is_empty());
    }
}
