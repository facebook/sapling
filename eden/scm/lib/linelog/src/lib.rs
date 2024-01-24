/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod linelog;
mod maybe_mut;

pub use crate::linelog::AbstractLineLog;

/// LineLog with string line content.
pub type LineLog = AbstractLineLog<String>;

#[cfg(test)]
mod tests;
