// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Logging support
//!
//! - The `blackbox` module provides building blocks for logging serializable
//!   objects, searching them by time range, etc. It is application agnostic.
//! - The `event` module assumes the source control application. It is not
//!   designed to be general purpose.

// This is a library exporting functions that seem "dead" if compiled alone.
#![allow(dead_code)]

mod blackbox;
mod singleton;

pub use self::blackbox::{Blackbox, BlackboxOptions, Entry, Filter};
pub use self::singleton::{init, log, SINGLETON};

pub mod event;
