/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use mononoke_types::{ContentId, FsnodeId};
use thiserror::Error;

mod derive;
mod mapping;

pub use mapping::{RootFsnodeId, RootFsnodeMapping};

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Invalid bonsai changeset: {0}")]
    InvalidBonsai(String),
    #[error("Missing content: {0}")]
    MissingContent(ContentId),
    #[error("Missing fsnode parent: {0}")]
    MissingParent(FsnodeId),
    #[error("Missing fsnode subentry for '{0}': {1}")]
    MissingSubentry(String, FsnodeId),
}
