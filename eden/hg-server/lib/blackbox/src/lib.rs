/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Logging support
//!
//! - The `blackbox` module provides building blocks for logging serializable
//!   objects, searching them by time range, etc. It is application agnostic.
//! - The `event` module assumes the source control application. It is not
//!   designed to be general purpose.

// This is a library exporting functions that seem "dead" if compiled alone.
#![allow(dead_code)]

mod blackbox;
mod match_pattern;
mod singleton;

pub use self::blackbox::{Blackbox, BlackboxOptions, Entry, SessionId, ToValue};
pub use self::singleton::{init, log, sync, SINGLETON};
pub use match_pattern::{capture_pattern, match_pattern};
pub use serde_json::{self, json, Value};

pub mod event;
