/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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

pub use match_pattern::capture_pattern;
pub use match_pattern::match_pattern;
pub use serde_json;
pub use serde_json::json;
pub use serde_json::Value;

pub use self::blackbox::Blackbox;
pub use self::blackbox::BlackboxOptions;
pub use self::blackbox::Entry;
pub use self::blackbox::SessionId;
pub use self::blackbox::ToValue;
pub use self::singleton::init;
pub use self::singleton::log;
pub use self::singleton::sync;
pub use self::singleton::SINGLETON;

pub mod event;
