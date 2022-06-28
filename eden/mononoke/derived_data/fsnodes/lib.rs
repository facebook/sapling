/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::ContentId;
use mononoke_types::FsnodeId;
use thiserror::Error;

mod batch;
mod derive;
mod mapping;

pub use derive::prefetch_content_metadata;
pub use mapping::RootFsnodeId;

#[derive(Debug, Error)]
pub enum FsnodeDerivationError {
    #[error("Invalid bonsai changeset: {0}")]
    InvalidBonsai(String),
    #[error("Missing content: {0}")]
    MissingContent(ContentId),
    #[error("Missing fsnode parent: {0}")]
    MissingParent(FsnodeId),
    #[error("Missing fsnode subentry for '{0}': {1}")]
    MissingSubentry(String, FsnodeId),
}
