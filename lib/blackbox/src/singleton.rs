// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Per-process [`Blackbox`] singleton.
//!
//! Useful for cases where it's inconvenient to pass [`Blackbox`] around.

use crate::{Blackbox, BlackboxOptions};
use lazy_static::lazy_static;
use std::ops::DerefMut;
use std::sync::Mutex;

lazy_static! {
    pub static ref SINGLETON: Mutex<Blackbox> =
        Mutex::new(BlackboxOptions::new().create_in_memory().unwrap());
}

/// Replace the global [`Blackbox`] instance.
pub fn init(blackbox: Blackbox) {
    let mut singleton = SINGLETON.lock().unwrap();
    *singleton.deref_mut() = blackbox;
}

/// Log to the global [`Blackbox`] instance.
///
/// Do nothing if [`init`] was not called.
pub fn log(data: &impl serde::Serialize) {
    if let Ok(mut singleton) = SINGLETON.lock() {
        let blackbox = singleton.deref_mut();
        blackbox.log(data);
    }
}

/// Write buffered data to disk.
pub fn sync() {
    if let Ok(mut singleton) = SINGLETON.lock() {
        let blackbox = singleton.deref_mut();
        blackbox.sync();
    }
}
