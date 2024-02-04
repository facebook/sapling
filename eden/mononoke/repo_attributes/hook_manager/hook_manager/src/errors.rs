/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::ContentId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HookManagerError {
    #[error("No such hook '{0}'")]
    NoSuchHook(String),
}

#[derive(Debug, Error)]
pub enum HookFileContentProviderError {
    #[error("Content with id '{0}' not found")]
    ContentIdNotFound(ContentId),

    #[error("Content too large to fit in memory")]
    ContentTooLarge,

    #[error(transparent)]
    Error(#[from] anyhow::Error),
}

impl From<std::num::TryFromIntError> for HookFileContentProviderError {
    fn from(_: std::num::TryFromIntError) -> Self {
        Self::ContentTooLarge
    }
}
