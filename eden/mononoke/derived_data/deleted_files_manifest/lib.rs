/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod derive;
mod mapping;
mod mapping_v1;
mod mapping_v2;
mod ops;
#[cfg(test)]
pub mod test_utils;

pub use mapping_v1::RootDeletedManifestId;
pub use mapping_v2::RootDeletedManifestV2Id;
pub use ops::{DeletedManifestOps, PathState};
