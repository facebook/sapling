// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use cloned::cloned;
use failure_ext::Error;
use futures::{Future, IntoFuture};
use futures_ext::{asynchronize, try_boxfuture, BoxFuture, FutureExt};
use scribe::ScribeClient;
use scribe_cxx::ScribeCxxClient;
use serde_derive::Serialize;
use serde_json;

use mononoke_types::{BonsaiChangeset, ChangesetId, Generation, RepositoryId};

// This is a type system trick. Before we add the commit to the changesets table, we have all this
// information...
pub struct PreCommitInfo {
    repo_id: RepositoryId,
    changeset_id: ChangesetId,
    parents: Vec<ChangesetId>,
}

// ... and after we add the commit to the changesets table, we can add the generation number, too
// These are separate structs so that the type system enforces the conversion.
#[derive(Serialize)]
#[serde(rename = "changeset_info")]
pub struct PostCommitInfo {
    repo_id: RepositoryId,
    generation: Generation,
    changeset_id: ChangesetId,
    parents: Vec<ChangesetId>,
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
            parents: changeset.parents().collect(),
        }
    }

    pub fn get_changeset_id(&self) -> ChangesetId {
        self.changeset_id
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

pub struct LogToScribe<C>
where
    C: ScribeClient + Sync + Send + 'static,
{
    client: Arc<C>,
    category: Option<String>,
}

impl LogToScribe<ScribeCxxClient> {
    pub fn new_with_default_scribe(category: Option<String>) -> Self {
        Self {
            client: Arc::new(ScribeCxxClient::new()),
            category,
        }
    }
}

impl<C> LogToScribe<C>
where
    C: ScribeClient + Sync + Send + 'static,
{
    pub fn new(client: C, category: Option<String>) -> Self {
        Self {
            client: Arc::new(client),
            category,
        }
    }
}

impl<C> PostCommitQueue for LogToScribe<C>
where
    C: ScribeClient + Sync + Send + 'static,
{
    fn queue_commit(&self, pc: PostCommitInfo) -> BoxFuture<(), Error> {
        match &self.category {
            Some(category) => {
                let pc = try_boxfuture!(serde_json::to_string(&pc));
                self.client
                    .offer(category, &pc)
                    .into_future()
                    .or_else({
                        cloned!(self.client, category);
                        move |_| asynchronize(move || client.blocking_put(&category, &pc))
                    })
                    .from_err()
                    .boxify()
            }
            None => Ok(()).into_future().boxify(),
        }
    }
}
