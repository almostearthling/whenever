//! A module providing named mutexes that can be shared across all threads.
//!
//! This module implements a global set of named mutexes that can be locked
//! and released by any thread.

#![cfg(feature = "lua_sync")]
#![allow(dead_code)]

// NOTE: originally this module was AI generated, however the poor guy
// kept making mistakes of all sorts, from completely reinventing common
// (or standard) libraries API, to writing failing tests, and so on. In
// the end, however, it made me discover some interesting libraries and
// rethink on how to implement the functionality by hand. Now the module
// is hand coded (apart from doc comments)

use lazy_static::lazy_static;
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

// global map of named mutexes
#[derive(Debug)]
struct SharedLock {
    busy: Mutex<bool>,
    notifier: Condvar,
}

#[allow(dead_code)]
impl SharedLock {
    pub fn new_free() -> Self {
        SharedLock {
            busy: Mutex::new(false),
            notifier: Condvar::new(),
        }
    }

    // busy by default
    pub fn new() -> Self {
        SharedLock {
            busy: Mutex::new(true),
            notifier: Condvar::new(),
        }
    }

    // this reclaims a named mutex, possibly with a timeout: if able to
    // capture it, then it changes its busy state to true and returns
    // true to signal that it succeeded
    pub fn claim(&self, timeout: Option<Duration>) -> bool {
        let mut busy = self.busy.lock();
        if *busy {
            if let Some(timeout) = timeout {
                if self.notifier.wait_for(&mut busy, timeout).timed_out() {
                    false
                } else {
                    *busy = true;
                    true
                }
            } else {
                self.notifier.wait(&mut busy);
                *busy = true;
                true
            }
        } else {
            *busy = true;
            true
        }
    }

    // free the mutex and signal the next waiting thread that it can go on;
    // this fails only if there was nothing to release
    pub fn free(&self) -> bool {
        let mut busy = self.busy.lock();
        if *busy {
            *busy = false;
            self.notifier.notify_one();
            true
        } else {
            false
        }
    }
}

lazy_static! {
    static ref NMUTEX_MAP: Arc<Mutex<HashMap<String, Arc<SharedLock>>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

// add or retrieve a lock
fn get_slock(name: &str) -> Arc<SharedLock> {
    let map = NMUTEX_MAP.clone();
    let mut map = map.lock();
    let s1 = map
        .entry(name.to_string())
        .or_insert_with(|| Arc::new(SharedLock::new_free()));
    s1.clone()
}

// the actual library, as per specification

/// Attempts to acquire and lock a named mutex.
///
/// If a mutex with the given `name` doesn't exist, it creates one and
/// locks it immediately.     /// If a mutex with the given `name` exists,
/// it attempts to lock it.
///
/// # Arguments
///
/// * `name` - The name of the mutex to lock
/// * `timeout` - Maximum time to wait for the lock.
///   - `None`: Wait indefinitely
///   - `Some(duration)`: Wait for the specified duration
///
/// # Returns
///
/// Returns `true` if the mutex was successfully locked, `false` if the
/// timeout was exceeded.
///
/// # Examples
///
/// ```ignore
/// if namedmutex_lock("Mux01", None) {
///     println!("locked!");
///     std::thread::sleep(std::time::Duration::from_millis(500));
///     let _ = namedmutex_release("Mux01");
/// }
/// ```
///
/// ```ignore
/// if !namedmutex_lock("Mux01", Some(Duration::from_millis(1000))) {
///     println!("could not lock the mutex");
/// }
/// ```
pub fn namedmutex_lock(name: &str, timeout: Option<Duration>) -> bool {
    let sl = &mut get_slock(name);
    sl.claim(timeout)
}

/// Releases a previously locked named mutex.
///
/// # Arguments
///
/// * `name` - The name of the mutex to release
///
/// # Returns
///
/// Returns `true` if the mutex was successfully released, `false` if a
/// mutex with the specified name was not found or was not locked by the
/// current thread.
///
/// # Examples
///
/// ```ignore
/// if namedmutex_lock("Mux01", None) {
///     // ... do some work ...
///     namedmutex_release("Mux01");
/// }
/// ```
pub fn namedmutex_release(name: &str) -> bool {
    let sl = &mut get_slock(name);
    sl.free()
}
