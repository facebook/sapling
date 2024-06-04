/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use mononoke_types::ChangesetId;

#[derive(thiserror::Error, Debug)]
pub enum CasChangesetUploaderErrorKind {
    #[error("The following changeset is unexpectedly missing: {0}")]
    InvalidChangeset(ChangesetId),
    #[error(transparent)]
    Error(#[from] Error),
}
