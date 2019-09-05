// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use futures_ext::BoxFuture;

use apiserver_thrift::client::{make_MononokeAPIService, MononokeAPIService};
use apiserver_thrift::types::{
    MononokeBlob, MononokeBranches, MononokeChangeset, MononokeDirectory, MononokeDirectoryUnodes,
    MononokeGetBlobParams, MononokeGetBranchesParams, MononokeGetChangesetParams,
    MononokeGetLastCommitOnPathParams, MononokeGetRawParams, MononokeGetTreeParams,
    MononokeIsAncestorParams, MononokeListDirectoryParams, MononokeListDirectoryUnodesParams,
    MononokeNodeHash, MononokeRevision, MononokeTreeHash,
};
use srclient::SRChannelBuilder;

pub struct MononokeAPIClient {
    inner: Arc<dyn MononokeAPIService + Send + Sync + 'static>,
    repo: String,
}

impl MononokeAPIClient {
    pub fn new_with_tier_repo(tier: &str, repo: &str) -> Result<Self, failure_ext::Error> {
        let inner =
            SRChannelBuilder::from_service_name(tier)?.build_client(make_MononokeAPIService)?;

        Ok(Self {
            inner,
            repo: repo.to_string(),
        })
    }

    pub fn get_raw(
        &self,
        revision: String,
        path: String,
        bookmark: bool,
    ) -> BoxFuture<Vec<u8>, failure_ext::Error> {
        let rev = if bookmark {
            MononokeRevision::bookmark(revision)
        } else {
            MononokeRevision::commit_hash(revision)
        };

        self.inner.get_raw(&MononokeGetRawParams {
            repo: self.repo.clone(),
            revision: rev,
            path: path.into_bytes(),
        })
    }

    pub fn get_changeset(
        &self,
        revision: String,
    ) -> BoxFuture<MononokeChangeset, failure_ext::Error> {
        self.inner.get_changeset(&MononokeGetChangesetParams {
            repo: self.repo.clone(),
            revision: MononokeRevision::commit_hash(revision),
        })
    }

    pub fn get_branches(&self) -> BoxFuture<MononokeBranches, failure_ext::Error> {
        self.inner.get_branches(&MononokeGetBranchesParams {
            repo: self.repo.clone(),
        })
    }

    pub fn get_last_commit_on_path(
        &self,
        revision: String,
        path: String,
    ) -> BoxFuture<MononokeChangeset, failure_ext::Error> {
        self.inner
            .get_last_commit_on_path(&MononokeGetLastCommitOnPathParams {
                repo: self.repo.clone(),
                revision: MononokeRevision::commit_hash(revision),
                path: path.into_bytes(),
            })
    }

    pub fn list_directory(
        &self,
        revision: String,
        path: String,
    ) -> BoxFuture<MononokeDirectory, failure_ext::Error> {
        self.inner.list_directory(&MononokeListDirectoryParams {
            repo: self.repo.clone(),
            revision: MononokeRevision::commit_hash(revision),
            path: path.into_bytes(),
        })
    }

    pub fn list_directory_unodes(
        &self,
        revision: String,
        path: String,
    ) -> BoxFuture<MononokeDirectoryUnodes, failure_ext::Error> {
        self.inner
            .list_directory_unodes(&MononokeListDirectoryUnodesParams {
                repo: self.repo.clone(),
                revision: MononokeRevision::commit_hash(revision),
                path: path.into_bytes(),
            })
    }

    pub fn is_ancestor(
        &self,
        ancestor: String,
        descendant: String,
    ) -> BoxFuture<bool, failure_ext::Error> {
        self.inner.is_ancestor(&MononokeIsAncestorParams {
            repo: self.repo.clone(),
            ancestor: MononokeRevision::commit_hash(ancestor),
            descendant: MononokeRevision::commit_hash(descendant),
        })
    }

    pub fn get_blob(&self, hash: String) -> BoxFuture<MononokeBlob, failure_ext::Error> {
        self.inner.get_blob(&MononokeGetBlobParams {
            repo: self.repo.clone(),
            blob_hash: MononokeNodeHash { hash },
        })
    }

    pub fn get_tree(&self, hash: String) -> BoxFuture<MononokeDirectory, failure_ext::Error> {
        self.inner.get_tree(&MononokeGetTreeParams {
            repo: self.repo.clone(),
            tree_hash: MononokeTreeHash { hash },
        })
    }
}
