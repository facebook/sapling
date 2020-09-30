/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! progress - A generic interface for representing progress bars.
//!
//! The intention of this crate is to abstract away the implementation details
//! of progress bars from Mercurial's Rust code. This allows pure Rust code to
//! work with Mercurial's Python progress bars, but leaves the door open for
//! a pure Rust progress bar implementation in the future.

use anyhow::Result;

/// Factory interface for creating progress bars and spinners.
pub trait ProgressFactory: Send + Sync + 'static {
    /// Start a progress bar.
    fn bar(&self, message: &str, total: Option<u64>, unit: Unit) -> Result<Box<dyn ProgressBar>>;

    /// Start a progress spinner.
    fn spinner(&self, message: &str) -> Result<Box<dyn ProgressSpinner>>;
}

/// Trait to be implemented by types representing progress bars.
pub trait ProgressBar: Send + Sync + 'static {
    /// Get the current position of the progress bar.
    fn position(&self) -> Result<u64>;

    /// Get the total length of the progress bar.
    fn total(&self) -> Result<Option<u64>>;

    /// Set the current position of the progress bar.
    fn set(&self, pos: u64) -> Result<()>;

    /// Change the total length of the progress bar.
    fn set_total(&self, total: Option<u64>) -> Result<()>;

    /// Increment the current position by the given amount.
    fn increment(&self, delta: u64) -> Result<()>;

    /// Change the message shown by the progress bar.
    fn set_message(&self, message: &str) -> Result<()>;
}

/// Trait to be implemented by types representing progress spinners.
pub trait ProgressSpinner: Send + Sync + 'static {
    /// Change the message shown by the spinner.
    fn set_message(&self, message: &str) -> Result<()>;
}

/// Unit to display in progress bar's counter.
#[derive(Copy, Clone, Debug)]
pub enum Unit<'a> {
    None,
    Bytes,
    Named(&'a str),
}

impl<'a> Default for Unit<'a> {
    fn default() -> Self {
        Unit::None
    }
}

impl<'a> From<&'a str> for Unit<'a> {
    fn from(unit: &'a str) -> Self {
        match unit {
            "" => Unit::None,
            "bytes" => Unit::Bytes,
            other => Unit::Named(other),
        }
    }
}
