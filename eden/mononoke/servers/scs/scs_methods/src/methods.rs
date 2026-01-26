/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_requests::types::AsyncPingToken;
use context::CoreContext;
use itertools::Itertools;
use metaconfig_types::CommitIdentityScheme;
use source_control as thrift;

use crate::async_requests::enqueue;
use crate::async_requests::poll;
use crate::from_request::FromRequest;
use crate::source_control_impl::SourceControlServiceImpl;

pub(crate) mod cloud;
pub(crate) mod commit;
pub(crate) mod commit_lookup_pushrebase_history;
pub(crate) mod commit_path;
pub(crate) mod commit_restricted_paths;
pub mod commit_sparse_profile_info;
pub(crate) mod create_repos;
pub(crate) mod file;
pub(crate) mod git;
pub(crate) mod megarepo;
pub(crate) mod repo;
pub(crate) mod tree;

impl SourceControlServiceImpl {
    pub(crate) async fn list_repos(
        &self,
        _ctx: CoreContext,
        params: thrift::ListReposParams,
    ) -> Result<Vec<thrift::Repo>, scs_errors::ServiceError> {
        let names = self.mononoke.repo_names_in_tier.iter();
        let names: Box<dyn Iterator<Item = _>> =
            if let Some(identity_schemes) = params.identity_schemes {
                let schemes = identity_schemes
                    .iter()
                    .map(CommitIdentityScheme::from_request)
                    .collect::<Result<Vec<_>, _>>()?;

                Box::new(names.filter(move |(_, default_scheme)| schemes.contains(default_scheme)))
            } else {
                Box::new(names)
            };

        Ok(names
            .sorted_by(|(a, _), (b, _)| a.cmp(b))
            .map(|(repo_name, _)| thrift::Repo {
                name: repo_name.clone(),
                ..Default::default()
            })
            .collect())
    }

    pub(crate) async fn async_ping(
        &self,
        ctx: CoreContext,
        params: thrift::AsyncPingParams,
    ) -> Result<thrift::AsyncPingToken, scs_errors::ServiceError> {
        enqueue::<thrift::AsyncPingParams>(&ctx, &self.async_requests_queue, None, params).await
    }

    pub(crate) async fn async_ping_poll(
        &self,
        ctx: CoreContext,
        token: thrift::AsyncPingToken,
    ) -> Result<thrift::AsyncPingPollResponse, scs_errors::ServiceError> {
        let token = AsyncPingToken(token);
        poll::<AsyncPingToken>(&ctx, &self.async_requests_queue, token).await
    }
}
