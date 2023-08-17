/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Error;
use mononoke_types::RepositoryId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DerivationError {
    #[error("Derivation of {0} is not enabled for repo={2} repoid={1}")]
    Disabled(&'static str, RepositoryId, String),
    #[error("Derivation of {0} timed out after {1:?}")]
    Timeout(&'static str, Duration),
    #[error("Derivation of {0} failed after {1} attempts")]
    Failed(&'static str, u32, #[source] Error),
    #[error(transparent)]
    Error(#[from] Error),
}
