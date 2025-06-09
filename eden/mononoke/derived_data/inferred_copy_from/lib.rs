/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod derive;
mod mapping;
pub mod similarity;
#[cfg(test)]
mod tests;

pub use mapping::RootInferredCopyFromId;
pub use mapping::format_key;
