//! Locking that survives a panic elsewhere.
//!
//! `Mutex::lock().unwrap()` turns one panicking thread (while it held the lock)
//! into a panic in every later caller: a bug in one request handler would take
//! the indexer status, the job list or the activity log down with it for good.
//! The state behind these locks (status snapshots, job maps, small caches) stays
//! valid after a panic, so these helpers take the guard back from a poisoned lock
//! and carry on instead.

use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub trait MutexExt<T> {
    /// Lock, ignoring poisoning.
    fn locked(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn locked(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub trait RwLockExt<T> {
    /// Read-lock, ignoring poisoning.
    fn read_locked(&self) -> RwLockReadGuard<'_, T>;
    /// Write-lock, ignoring poisoning.
    fn write_locked(&self) -> RwLockWriteGuard<'_, T>;
}

impl<T> RwLockExt<T> for RwLock<T> {
    fn read_locked(&self) -> RwLockReadGuard<'_, T> {
        self.read().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    fn write_locked(&self) -> RwLockWriteGuard<'_, T> {
        self.write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn a_mutex_poisoned_by_a_panic_is_still_usable() {
        let m = Arc::new(Mutex::new(1));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("die holding the lock");
        })
        .join();
        assert!(m.is_poisoned());
        assert_eq!(*m.locked(), 1);
        *m.locked() = 2;
        assert_eq!(*m.locked(), 2);
    }

    #[test]
    fn an_rwlock_poisoned_by_a_panic_is_still_usable() {
        let l = Arc::new(RwLock::new(String::from("a")));
        let l2 = l.clone();
        let _ = std::thread::spawn(move || {
            let _g = l2.write().unwrap();
            panic!("die holding the lock");
        })
        .join();
        assert!(l.is_poisoned());
        assert_eq!(*l.read_locked(), "a");
        l.write_locked().push('b');
        assert_eq!(*l.read_locked(), "ab");
    }
}
