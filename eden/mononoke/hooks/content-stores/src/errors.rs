/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::ContentId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Content with id '{0}' not found")]
    ContentIdNotFound(ContentId),
    #[error(transparent)]
    BackingStore(#[from] anyhow::Error),
    #[error("Content too large to fit in memory")]
    ContentTooLarge,
}

impl From<std::num::TryFromIntError> for ErrorKind {
    fn from(_: std::num::TryFromIntError) -> Self {
        Self::ContentTooLarge
    }
}
