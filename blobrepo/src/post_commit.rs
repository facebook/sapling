// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Error;
use futures::IntoFuture;
use futures_ext::{BoxFuture, FutureExt};

use mercurial_types::RepositoryId;
use mononoke_types::{BonsaiChangeset, ChangesetId, Generation};

// This is a type system trick. Before we add the commit to the changesets table, we have all this
// information...
pub struct PreCommitInfo {
    repo_id: RepositoryId,
    changeset_id: ChangesetId,
    parents: Vec<ChangesetId>,
}

// ... and after we add the commit to the changesets table, we can add the generation number, too
// These are separate structs so that the type system enforces the conversion.
pub struct PostCommitInfo {
    pub repo_id: RepositoryId,
    pub generation: Generation,
    pub changeset_id: ChangesetId,
    pub parents: Vec<ChangesetId>,
}

impl PreCommitInfo {
    pub fn new(
        repo_id: RepositoryId,
        changeset_id: ChangesetId,
        changeset: &BonsaiChangeset,
    ) -> Self {
        Self {
            repo_id,
            changeset_id,
            parents: changeset.parents().cloned().collect(),
        }
    }

    pub fn get_changeset_id(&self) -> &ChangesetId {
        &self.changeset_id
    }

    pub fn complete(self, generation: Generation) -> PostCommitInfo {
        // This is a trick to ensure that all fields are read out of PreCommitInfo - there will be
        // a compile error if you don't bind them all, and hopefully that compile error will make
        // you fix PostCommitInfo to match
        let Self {
            repo_id,
            changeset_id,
            parents,
        } = self;
        PostCommitInfo {
            repo_id,
            generation,
            changeset_id,
            parents,
        }
    }
}

pub trait PostCommitQueue: Send + Sync {
    fn queue_commit(&self, pc: PostCommitInfo) -> BoxFuture<(), Error>;
}

pub struct Discard {}

impl Discard {
    pub fn new() -> Self {
        Self {}
    }
}

impl PostCommitQueue for Discard {
    fn queue_commit(&self, _pc: PostCommitInfo) -> BoxFuture<(), Error> {
        Ok(()).into_future().boxify()
    }
}
