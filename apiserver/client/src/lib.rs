// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate apiserver_thrift;
extern crate futures_ext;
extern crate srclient;

use std::sync::Arc;

use futures_ext::BoxFuture;

use apiserver_thrift::client::{make_MononokeAPIService, MononokeAPIService};
use apiserver_thrift::types::{
    MononokeBranches, MononokeChangeset, MononokeGetBranchesParams, MononokeGetChangesetParams,
    MononokeGetRawParams,
};
use srclient::SRChannelBuilder;

pub struct MononokeAPIClient {
    inner: Arc<MononokeAPIService + Send + Sync + 'static>,
    repo: String,
}

impl MononokeAPIClient {
    pub fn new_with_tier_repo(tier: &str, repo: &str) -> Result<Self, srclient::errors::Error> {
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
    ) -> BoxFuture<Vec<u8>, apiserver_thrift::errors::Error> {
        self.inner.get_raw(&MononokeGetRawParams {
            repo: self.repo.clone(),
            changeset: revision,
            path: path.into_bytes(),
        })
    }

    pub fn get_changeset(
        &self,
        revision: String,
    ) -> BoxFuture<MononokeChangeset, apiserver_thrift::errors::Error> {
        self.inner.get_changeset(&MononokeGetChangesetParams {
            repo: self.repo.clone(),
            revision,
        })
    }

    pub fn get_branches(&self) -> BoxFuture<MononokeBranches, apiserver_thrift::errors::Error> {
        self.inner.get_branches(&MononokeGetBranchesParams {
            repo: self.repo.clone(),
        })
    }
}
