/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::sync::Arc;

use anyhow::Error;
use fbinit::FacebookInit;
use futures::{Future, IntoFuture};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use scribe::ScribeClient;
use scribe_cxx::ScribeCxxClient;
use serde_derive::Serialize;

use mononoke_types::{ChangesetId, Generation, RepositoryId};

#[derive(Serialize)]
pub struct CommitInfo {
    repo_id: RepositoryId,
    generation: Generation,
    changeset_id: ChangesetId,
    parents: Vec<ChangesetId>,
}

impl CommitInfo {
    pub fn new(
        repo_id: RepositoryId,
        generation: Generation,
        changeset_id: ChangesetId,
        parents: Vec<ChangesetId>,
    ) -> Self {
        Self {
            repo_id,
            generation,
            changeset_id,
            parents,
        }
    }
}

pub trait ScribeCommitQueue: Send + Sync {
    fn queue_commit(&self, commit: CommitInfo) -> BoxFuture<(), Error>;
}

pub struct LogToScribe<C>
where
    C: ScribeClient + Sync + Send + 'static,
{
    client: Option<Arc<C>>,
    category: String,
}

impl LogToScribe<ScribeCxxClient> {
    pub fn new_with_default_scribe(fb: FacebookInit, category: String) -> Self {
        Self {
            client: Some(Arc::new(ScribeCxxClient::new(fb))),
            category,
        }
    }

    pub fn new_with_discard() -> Self {
        Self {
            client: None,
            category: String::new(),
        }
    }
}

impl<C> LogToScribe<C>
where
    C: ScribeClient + Sync + Send + 'static,
{
    pub fn new(client: C, category: String) -> Self {
        Self {
            client: Some(Arc::new(client)),
            category,
        }
    }
}

impl<C> ScribeCommitQueue for LogToScribe<C>
where
    C: ScribeClient + Sync + Send + 'static,
{
    fn queue_commit(&self, commit: CommitInfo) -> BoxFuture<(), Error> {
        match &self.client {
            Some(client) => {
                let commit = try_boxfuture!(serde_json::to_string(&commit));
                client
                    .offer(&self.category, &commit)
                    .into_future()
                    .from_err()
                    .boxify()
            }
            None => Ok(()).into_future().boxify(),
        }
    }
}
