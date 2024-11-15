/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Progress rendering.

mod config;
pub mod nodeipc;
pub mod simple;
pub mod structured;
mod unit;

use std::fmt::Display;

pub use config::RenderingConfig;

#[cfg(test)]
mod tests;

pub(crate) fn maybe_pad<S: AsRef<str> + Display>(s: S) -> PadIfNonEmpty<S> {
    PadIfNonEmpty { s }
}

pub(crate) struct PadIfNonEmpty<S: AsRef<str> + Display> {
    s: S,
}

impl<S: AsRef<str> + Display> Display for PadIfNonEmpty<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.s.as_ref().is_empty() {
            Ok(())
        } else {
            f.write_str(" ")?;
            self.s.fmt(f)
        }
    }
}
