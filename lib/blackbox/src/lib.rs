// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Logging support

// This is a library exporting functions that seem "dead" if compiled alone.
#![allow(dead_code)]

mod blackbox;

pub use self::blackbox::{Blackbox, BlackboxOptions, Entry, Filter};
