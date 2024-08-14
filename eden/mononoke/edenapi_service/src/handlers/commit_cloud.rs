/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use edenapi_types::CloudShareWorkspaceRequest;
use edenapi_types::CloudShareWorkspaceResponse;
use edenapi_types::CloudWorkspaceRequest;
use edenapi_types::CloudWorkspacesRequest;
use edenapi_types::GetReferencesParams;
use edenapi_types::GetSmartlogParams;
use edenapi_types::ReferencesDataResponse;
use edenapi_types::ServerError;
use edenapi_types::SmartlogDataResponse;
use edenapi_types::UpdateReferencesParams;
use edenapi_types::WorkspaceDataResponse;
use edenapi_types::WorkspacesDataResponse;
use futures::stream;
use futures::FutureExt;
use futures::StreamExt;
use mononoke_api_hg::HgRepoContext;

use super::handler::SaplingRemoteApiContext;
use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;
pub struct CommitCloudWorkspace;
pub struct CommitCloudWorkspaces;
pub struct CommitCloudReferences;
pub struct CommitCloudUpdateReferences;
pub struct CommitCloudSmartlog;
pub struct CommitCloudShareWorkspace;

#[async_trait]
impl SaplingRemoteApiHandler for CommitCloudWorkspace {
    type Request = CloudWorkspaceRequest;
    type Response = WorkspaceDataResponse;

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
) -> anyhow::Result<WorkspaceDataResponse> {
    Ok(WorkspaceDataResponse {
        data: repo
            .cloud_workspace(&request.workspace, &request.reponame)
            .await
            .map_err(ServerError::from),
    })
}

#[async_trait]
impl SaplingRemoteApiHandler for CommitCloudWorkspaces {
    type Request = CloudWorkspacesRequest;
    type Response = WorkspacesDataResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::CloudWorkspaces;
    const ENDPOINT: &'static str = "/cloud/workspaces";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = get_workspaces(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn get_workspaces(
    request: CloudWorkspacesRequest,
    repo: HgRepoContext,
) -> anyhow::Result<WorkspacesDataResponse> {
    Ok(WorkspacesDataResponse {
        data: repo
            .cloud_workspaces(&request.prefix, &request.reponame)
            .await
            .map_err(ServerError::from),
    })
}

#[async_trait]
impl SaplingRemoteApiHandler for CommitCloudReferences {
    type Request = GetReferencesParams;
    type Response = ReferencesDataResponse;

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
) -> anyhow::Result<ReferencesDataResponse, Error> {
    Ok(ReferencesDataResponse {
        data: repo
            .cloud_references(&request)
            .await
            .map_err(ServerError::from),
    })
}

#[async_trait]
impl SaplingRemoteApiHandler for CommitCloudUpdateReferences {
    type Request = UpdateReferencesParams;
    type Response = ReferencesDataResponse;

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
) -> anyhow::Result<ReferencesDataResponse, Error> {
    Ok(ReferencesDataResponse {
        data: repo
            .cloud_update_references(&request)
            .await
            .map_err(ServerError::from),
    })
}

#[async_trait]
impl SaplingRemoteApiHandler for CommitCloudSmartlog {
    type Request = GetSmartlogParams;
    type Response = SmartlogDataResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::CloudSmartlog;
    const ENDPOINT: &'static str = "/cloud/smartlog";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = get_smartlog(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn get_smartlog(
    request: GetSmartlogParams,
    repo: HgRepoContext,
) -> anyhow::Result<SmartlogDataResponse, Error> {
    Ok(SmartlogDataResponse {
        data: repo
            .cloud_smartlog(&request)
            .await
            .map_err(ServerError::from),
    })
}

#[async_trait]
impl SaplingRemoteApiHandler for CommitCloudShareWorkspace {
    type Request = CloudShareWorkspaceRequest;
    type Response = CloudShareWorkspaceResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::CloudShareWorkspace;
    const ENDPOINT: &'static str = "/cloud/share_workspace";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = share_workspace(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn share_workspace(
    request: CloudShareWorkspaceRequest,
    repo: HgRepoContext,
) -> anyhow::Result<CloudShareWorkspaceResponse, Error> {
    Ok(CloudShareWorkspaceResponse {
        data: repo
            .cloud_share_workspace(&request)
            .await
            .map_err(ServerError::from),
    })
}
