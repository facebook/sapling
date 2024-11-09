/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

mod derive;
mod mapping;

pub use crate::mapping::RootContentManifestId;

#[derive(Debug, Error)]
pub enum ContentManifestDerivationError {
    #[error("Invalid bonsai changeset: incomplete change with no parents")]
    NoParents,

    #[error("Invalid bonsai changeset: merge conflict not resolved")]
    MergeConflictNotResolved,
}
