// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::{Arc, Mutex, MutexGuard};

use futures::{Future, Stream};
use juniper::{Context, FieldResult};

use mercurial::RevlogRepo;

use changeset::GQLChangeset;
use node::GQLNodeId;

#[derive(Debug)]
pub struct GQLRepo;

impl GQLRepo {
    pub fn new() -> GQLRepo {
        GQLRepo
    }
}

#[derive(Clone)]
pub struct RepoCtx {
    repo: Arc<Mutex<RevlogRepo>>,
}

impl RepoCtx {
    pub fn new(repo: RevlogRepo) -> Self {
        RepoCtx { repo: Arc::new(Mutex::new(repo)) }
    }

    pub fn repo<'a>(&'a self) -> MutexGuard<'a, RevlogRepo> {
        self.repo.lock().expect("lock failed")
    }
}

impl Context for RepoCtx {}

graphql_object!(GQLRepo: RepoCtx as "Repo" |&self| {
    description: "A source control repository"

    field required(&executor) -> Vec<String> {
        let repo = executor.context().repo();
        repo.get_requirements().iter().map(|r| format!("{}", r)).collect()
    }

    // XXX replace with more general revset operator
    field heads(&executor) -> FieldResult<Vec<GQLChangeset>>
            as "Set of head revisions in the repo" {
        let mut repo = executor.context().repo();
        repo.get_heads()
            .collect()
            .wait() // TODO(jsgf) make async
            .map(|set| set.into_iter().map(GQLChangeset::from).collect())
            .map_err(From::from)
    }

    field changeset(&executor, id: GQLNodeId) -> FieldResult<GQLChangeset>
            as "Fetch a specific changeset" {
        let mut repo = executor.context().repo();
        // Check the id exists, but we don't need the result now
        if let Err(err) = repo.changeset_exists(&id).wait() /* TODO(jsgf) make async */ {
            return Err(String::from(err));
        }
        Ok(From::from(id))
    }

    field changesets(&executor) -> Vec<GQLChangeset>
            as "Fetch all changesets" {
        let mut repo = executor.context().repo();
        repo.changesets()
            .collect().wait().unwrap().into_iter() // TODO(jsgf) make async
            .map(GQLChangeset::from)
            .collect()
    }
});
