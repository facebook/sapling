/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use edenapi_types::cloud::WorkspaceData;
use edenapi_types::CloudWorkspaceRequest;
use edenapi_types::GetReferencesParams;
use edenapi_types::ReferencesData;
use edenapi_types::UpdateReferencesParams;
use futures::stream;
use futures::FutureExt;
use futures::StreamExt;
use mononoke_api_hg::HgRepoContext;

use super::handler::SaplingRemoteApiContext;
use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;
use crate::errors::ErrorKind;
pub struct CommitCloudWorkspace;
pub struct CommitCloudReferences;
pub struct CommitCloudUpdateReferences;

#[async_trait]
impl SaplingRemoteApiHandler for CommitCloudWorkspace {
    type Request = CloudWorkspaceRequest;
    type Response = WorkspaceData;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::CloudWorkspace;
    const ENDPOINT: &'static str = "/cloud/workspace";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = get_workspace(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn get_workspace(
    request: CloudWorkspaceRequest,
    repo: HgRepoContext,
) -> anyhow::Result<WorkspaceData> {
    let version = repo
        .cloud_workspace(&request.workspace, &request.reponame)
        .await?;
    if !version.is_empty() {
        let data = WorkspaceData {
            name: request.workspace,
            reponame: request.reponame,
            version: version[0].version,
            archived: version[0].archived,
            timestamp: version[0].timestamp.timestamp_nanos(),
        };
        return Ok(data);
    }
    Err(anyhow::anyhow!(ErrorKind::CloudWorkspaceNotFound(
        request.workspace
    )))
}

#[async_trait]
impl SaplingRemoteApiHandler for CommitCloudReferences {
    type Request = GetReferencesParams;
    type Response = ReferencesData;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::CloudReferences;
    const ENDPOINT: &'static str = "/cloud/references";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = get_references(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn get_references(
    request: GetReferencesParams,
    repo: HgRepoContext,
) -> anyhow::Result<ReferencesData> {
    Ok(repo.cloud_references(request).await?)
}

#[async_trait]
impl SaplingRemoteApiHandler for CommitCloudUpdateReferences {
    type Request = UpdateReferencesParams;
    type Response = ReferencesData;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::CloudUpdateReferences;
    const ENDPOINT: &'static str = "/cloud/update_references";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = update_references(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn update_references(
    request: UpdateReferencesParams,
    repo: HgRepoContext,
) -> anyhow::Result<ReferencesData> {
    Ok(repo.cloud_update_references(request).await?)
}
