/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod derive;
mod mapping;
mod ops;
#[cfg(test)]
pub mod test_utils;

pub use mapping::RootDeletedManifestId;
pub use ops::{DeletedManifestOps, PathState};
