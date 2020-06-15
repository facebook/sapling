/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(fbcode_build)]
mod facebook;
#[cfg(not(fbcode_build))]
mod oss;

#[cfg(fbcode_build)]
pub use crate::facebook::LogToScribe;
#[cfg(not(fbcode_build))]
pub use crate::oss::LogToScribe;

use anyhow::Error;
use async_trait::async_trait;
use mononoke_types::{ChangesetId, Generation, RepositoryId};
use serde_derive::Serialize;

#[derive(Serialize)]
pub struct CommitInfo<'a> {
    repo_id: RepositoryId,
    #[serde(skip_serializing_if = "Option::is_none")]
    bookmark: Option<&'a str>,
    generation: Generation,
    changeset_id: ChangesetId,
    parents: Vec<ChangesetId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_unix_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_hostname: Option<&'a str>,
}

impl<'a> CommitInfo<'a> {
    pub fn new(
        repo_id: RepositoryId,
        bookmark: Option<&'a str>,
        generation: Generation,
        changeset_id: ChangesetId,
        parents: Vec<ChangesetId>,
        user_unix_name: Option<&'a str>,
        source_hostname: Option<&'a str>,
    ) -> Self {
        Self {
            repo_id,
            bookmark,
            generation,
            changeset_id,
            parents,
            user_unix_name,
            source_hostname,
        }
    }
}

#[async_trait]
pub trait ScribeCommitQueue: Send + Sync {
    async fn queue_commit(&self, commit: &CommitInfo<'_>) -> Result<(), Error>;
}
