/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::BonsaiGitMappingEntry;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AddGitMappingErrorKind {
    #[error(
        "Conflicting mapping {0:?} detected while inserting git mappings (tried inserting: {1:?})"
    )]
    Conflict(Option<BonsaiGitMappingEntry>, Vec<BonsaiGitMappingEntry>),
    #[error("Internal error occurred while inserting git mapping")]
    InternalError(#[source] anyhow::Error),
}

impl From<anyhow::Error> for AddGitMappingErrorKind {
    fn from(error: anyhow::Error) -> Self {
        AddGitMappingErrorKind::InternalError(error)
    }
}
