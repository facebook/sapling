/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(fbcode_build)]
mod facebook;

#[cfg(fbcode_build)]
pub use crate::facebook::LogToScribe;
#[cfg(not(fbcode_build))]
pub use crate::r#impl::LogToScribe;

use anyhow::Error;
use async_trait::async_trait;
use mononoke_types::{ChangesetId, Generation, RepositoryId};
use serde_derive::Serialize;

#[derive(Serialize)]
pub struct CommitInfo<'a> {
    repo_id: RepositoryId,
    bookmark: &'a str,
    generation: Generation,
    changeset_id: ChangesetId,
    parents: Vec<ChangesetId>,
}

impl<'a> CommitInfo<'a> {
    pub fn new(
        repo_id: RepositoryId,
        bookmark: &'a str,
        generation: Generation,
        changeset_id: ChangesetId,
        parents: Vec<ChangesetId>,
    ) -> Self {
        Self {
            repo_id,
            bookmark,
            generation,
            changeset_id,
            parents,
        }
    }
}

#[async_trait]
pub trait ScribeCommitQueue: Send + Sync {
    async fn queue_commit(&self, commit: &CommitInfo<'_>) -> Result<(), Error>;
}

#[cfg(not(fbcode_build))]
mod r#impl {
    use super::*;

    use fbinit::FacebookInit;

    pub struct LogToScribe {}

    impl LogToScribe {
        pub fn new_with_default_scribe(_fb: FacebookInit, _category: String) -> Self {
            Self {}
        }

        pub fn new_with_discard() -> Self {
            Self {}
        }
    }

    #[async_trait]
    impl ScribeCommitQueue for LogToScribe {
        async fn queue_commit(&self, _commit: &CommitInfo<'_>) -> Result<(), Error> {
            Ok(())
        }
    }
}
