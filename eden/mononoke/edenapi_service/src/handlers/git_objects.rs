/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use async_stream::try_stream;
use async_trait::async_trait;
use edenapi_types::GitObjectBytes;
use edenapi_types::GitObjectsRequest;
use edenapi_types::GitObjectsResponse;
use edenapi_types::ServerError;
use futures::StreamExt;
use git_types::fetch_git_object_bytes;
use git_types::GitIdentifier;
use git_types::HeaderState;
use gotham_ext::handler::SlapiCommitIdentityScheme;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api_hg::HgRepoContext;
use mononoke_types::hash::GitSha1;
use types::Id20;

use super::handler::SaplingRemoteApiContext;
use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;

pub struct GitObjectsHandler;

#[async_trait]
impl SaplingRemoteApiHandler for GitObjectsHandler {
    type Request = GitObjectsRequest;
    type Response = GitObjectsResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::GitObjects;
    const ENDPOINT: &'static str = "/git_objects";
    const SUPPORTED_FLAVOURS: &'static [SlapiCommitIdentityScheme] =
        &[SlapiCommitIdentityScheme::Git];

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();

        Ok(try_stream! {
            for oid in request.object_ids {
                let git_object = fetch_git_object(oid, &repo).await;
                yield GitObjectsResponse {
                    oid,
                    result: git_object.map_err(|e| ServerError::generic(format!("{}", e))),
                }
            }
        }
        .boxed())
    }
}

pub(crate) async fn fetch_git_object<R: MononokeRepo>(
    oid: Id20,
    repo: &HgRepoContext<R>,
) -> Result<GitObjectBytes, Error> {
    let git_identifier = GitIdentifier::Basic(GitSha1::from_bytes(oid.as_ref())?);
    let bytes = fetch_git_object_bytes(
        repo.ctx(),
        Arc::new(repo.repo_ctx().repo_blobstore().clone()),
        &git_identifier,
        HeaderState::Included,
    )
    .await?;

    Ok(GitObjectBytes {
        bytes: bytes.into(),
    })
}
