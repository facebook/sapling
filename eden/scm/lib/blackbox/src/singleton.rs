/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Per-process [`Blackbox`] singleton.
//!
//! Useful for cases where it's inconvenient to pass [`Blackbox`] around.

use std::ops::DerefMut;

use lazy_static::lazy_static;
use parking_lot::Mutex;

use crate::event::Event;
use crate::Blackbox;
use crate::BlackboxOptions;

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
    let old_blackbox = singleton.deref_mut();
    for entry in old_blackbox.log.iter_dirty() {
        if let Ok(entry) = entry {
            let _ = blackbox.log.append(entry);
        }
    }

    // Preserve session_id if pid hasn't been changed.
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

/// Reset the blackbox to an in-memory instance.
/// This releases file handlers on Windows so related files
/// can be deleted.
pub fn reset() {
    let blackbox = BlackboxOptions::new().create_in_memory().unwrap();
    init(blackbox);
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::blackbox::tests::all_entries;

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
