/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::SkeletonManifestId;
use thiserror::Error;

mod batch;
mod derive;
pub mod mapping;

pub use mapping::RootSkeletonManifestId;

#[derive(Debug, Error)]
pub enum SkeletonManifestDerivationError {
    #[error("Invalid bonsai changeset: {0}")]
    InvalidBonsai(String),
    #[error("Missing skeleton manifest parent: {0}")]
    MissingParent(SkeletonManifestId),
    #[error("Missing skeleton manifest subentry for '{0}': {1}")]
    MissingSubentry(String, SkeletonManifestId),
}
