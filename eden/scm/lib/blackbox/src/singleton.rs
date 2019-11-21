/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Per-process [`Blackbox`] singleton.
//!
//! Useful for cases where it's inconvenient to pass [`Blackbox`] around.

use crate::{event::Event, Blackbox, BlackboxOptions};
use indexedlog::rotate::RotateLowLevelExt;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use std::ops::{Deref, DerefMut};

lazy_static! {
    pub static ref SINGLETON: Mutex<Blackbox> =
        Mutex::new(BlackboxOptions::new().create_in_memory().unwrap());
}

/// Replace the global [`Blackbox`] instance.
///
/// The session ID of the old blackbox will be reused.
///
/// If [`log`] was called, their side effects will be re-applied to the
/// specified blackbox.
pub fn init(mut blackbox: Blackbox) {
    let mut singleton = SINGLETON.lock();

    // Insert dirty entries to the new blackbox.
    let old_blackbox = singleton.deref();
    for log in old_blackbox.log.logs().iter() {
        for entry in log.iter_dirty() {
            if let Ok(entry) = entry {
                let _ = blackbox.log.append(entry);
            }
        }
    }

    // Perserve session_id if pid hasn't been changed.
    if blackbox.session_pid() == old_blackbox.session_pid() {
        blackbox.session_id = old_blackbox.session_id;
    }

    *singleton.deref_mut() = blackbox;
}

/// Log to the global [`Blackbox`] instance.
///
/// If [`init`] was not called, log requests will be buffered in memory.
pub fn log(data: &Event) {
    SINGLETON.lock().log(data);
}

/// Write buffered data to disk.
pub fn sync() {
    SINGLETON.lock().sync();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackbox::tests::all_entries;
    use tempfile::tempdir;

    #[test]
    fn test_buffered_writes() {
        let events = vec![
            Event::Alias {
                from: "x".to_string(),
                to: "y".to_string(),
            },
            Event::Alias {
                from: "p".to_string(),
                to: "q".to_string(),
            },
        ];
        for e in &events {
            log(e);
        }

        let dir = tempdir().unwrap();
        let blackbox = BlackboxOptions::new().open(&dir).unwrap();
        init(blackbox);

        for e in &events[1..2] {
            log(e);
        }

        let mut singleton = SINGLETON.lock();
        let blackbox = singleton.deref_mut();
        assert_eq!(all_entries(blackbox).len(), 3);
    }

}
