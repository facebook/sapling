/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use edenapi_types::cloud::ClientInfo;
use edenapi_types::CloudShareWorkspaceRequest;
use edenapi_types::CloudShareWorkspaceResponse;
use edenapi_types::CloudWorkspaceRequest;
use edenapi_types::CloudWorkspacesRequest;
use edenapi_types::GetReferencesParams;
use edenapi_types::GetSmartlogByVersionParams;
use edenapi_types::GetSmartlogFlag;
use edenapi_types::GetSmartlogParams;
use edenapi_types::HistoricalVersion;
use edenapi_types::HistoricalVersionsData;
use edenapi_types::HistoricalVersionsParams;
use edenapi_types::HistoricalVersionsResponse;
use edenapi_types::ReferencesData;
use edenapi_types::ReferencesDataResponse;
use edenapi_types::RenameWorkspaceRequest;
use edenapi_types::RenameWorkspaceResponse;
use edenapi_types::ServerError;
use edenapi_types::SmartlogData;
use edenapi_types::SmartlogDataResponse;
use edenapi_types::UpdateArchiveParams;
use edenapi_types::UpdateArchiveResponse;
use edenapi_types::UpdateReferencesParams;
use edenapi_types::WorkspaceData;
use edenapi_types::WorkspaceDataResponse;
use edenapi_types::WorkspaceSharingData;
use edenapi_types::WorkspacesDataResponse;
use futures::stream;
use futures::FutureExt;
use futures::StreamExt;
use mononoke_api::MononokeError;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api_hg::HgRepoContext;

use super::handler::SaplingRemoteApiContext;
use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;
use crate::handlers::handler::PathExtractorWithRepo;
use crate::utils::commit_cloud_types::FromCommitCloudType;
use crate::utils::commit_cloud_types::IntoCommitCloudType;
pub struct CommitCloudWorkspace;
pub struct CommitCloudWorkspaces;
pub struct CommitCloudReferences;
pub struct CommitCloudUpdateReferences;
pub struct CommitCloudSmartlog;
pub struct CommitCloudSmartlogByVersion;
pub struct CommitCloudShareWorkspace;
pub struct CommitCloudRenameWorkspace;
pub struct CommitCloudUpdateArchive;
pub struct CommitCloudHistoricalVersions;

#[async_trait]
impl SaplingRemoteApiHandler for CommitCloudWorkspace {
    type Request = CloudWorkspaceRequest;
    type Response = WorkspaceDataResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::CloudWorkspace;
    const ENDPOINT: &'static str = "/cloud/workspace";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = get_workspace(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn get_workspace<R: MononokeRepo>(
    request: CloudWorkspaceRequest,
    repo: HgRepoContext<R>,
) -> anyhow::Result<WorkspaceDataResponse> {
    let cc_res = repo
        .cloud_workspace(&request.workspace, &request.reponame)
        .await;

    let res = match cc_res {
        Ok(res) => Ok(WorkspaceData::from_cc_type(res)?),
        Err(e) => Err(e),
    };
    Ok(WorkspaceDataResponse {
        data: res.map_err(ServerError::from),
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
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = get_workspaces(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn get_workspaces<R: MononokeRepo>(
    request: CloudWorkspacesRequest,
    repo: HgRepoContext<R>,
) -> anyhow::Result<WorkspacesDataResponse> {
    let cc_res = repo
        .cloud_workspaces(&request.prefix, &request.reponame)
        .await;
    let res = match cc_res {
        Ok(res) => Ok(res
            .into_iter()
            .map(WorkspaceData::from_cc_type)
            .collect::<anyhow::Result<Vec<_>>>()?),
        Err(e) => Err(e),
    };

    Ok(WorkspacesDataResponse {
        data: res.map_err(ServerError::from),
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
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = if ectx.path().repo() == request.reponame {
            ectx.repo()
        } else {
            ectx.other_repo(&request.reponame).await?
        };
        let res = get_references(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn get_references<R: MononokeRepo>(
    request: GetReferencesParams,
    repo: HgRepoContext<R>,
) -> anyhow::Result<ReferencesDataResponse, Error> {
    let ci = request
        .client_info
        .map(ClientInfo::into_cc_type)
        .transpose()?;
    let cc_res = repo
        .cloud_references(&request.workspace, &request.reponame, request.version, ci)
        .await;
    let res = match cc_res {
        Ok(res) => Ok(ReferencesData::from_cc_type(res)?),
        Err(e) => {
            match e {
                MononokeError::InternalError(ref e) => repo.ctx().scuba().clone().log_with_msg(
                    "commit cloud: 'get references' returned internal error",
                    Some(e.to_string()),
                ),
                _ => (),
            };
            Err(e)
        }
    };
    Ok(ReferencesDataResponse {
        data: res.map_err(ServerError::from),
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
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = update_references(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn update_references<R: MononokeRepo>(
    request: UpdateReferencesParams,
    repo: HgRepoContext<R>,
) -> anyhow::Result<ReferencesDataResponse, Error> {
    let cc_params = request.into_cc_type()?;
    let cc_res = repo.cloud_update_references(&cc_params).await;
    let res = match cc_res {
        Ok(res) => Ok(ReferencesData::from_cc_type(res)?),
        Err(e) => {
            match e {
                MononokeError::InternalError(ref e) => repo.ctx().scuba().clone().log_with_msg(
                    "commit cloud: 'update references' returned internal error",
                    Some(e.to_string()),
                ),
                _ => (),
            };
            Err(e)
        }
    };
    Ok(ReferencesDataResponse {
        data: res.map_err(ServerError::from),
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
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = get_smartlog(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn get_smartlog<R: MononokeRepo>(
    request: GetSmartlogParams,
    repo: HgRepoContext<R>,
) -> anyhow::Result<SmartlogDataResponse, Error> {
    let flags = request
        .flags
        .into_iter()
        .map(GetSmartlogFlag::into_cc_type)
        .collect::<anyhow::Result<Vec<_>>>()?;
    let cc_res = repo
        .cloud_smartlog(&request.workspace, &request.reponame, &flags)
        .await;
    let res = match cc_res {
        Ok(res) => Ok(SmartlogData::from_cc_type(res)?),
        Err(e) => Err(e),
    };
    Ok(SmartlogDataResponse {
        data: res.map_err(ServerError::from),
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
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = share_workspace(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn share_workspace<R: MononokeRepo>(
    request: CloudShareWorkspaceRequest,
    repo: HgRepoContext<R>,
) -> anyhow::Result<CloudShareWorkspaceResponse, Error> {
    let cc_res = repo
        .cloud_share_workspace(&request.workspace, &request.reponame)
        .await;
    let res = match cc_res {
        Ok(res) => Ok(WorkspaceSharingData::from_cc_type(res)?),
        Err(e) => Err(e),
    };
    Ok(CloudShareWorkspaceResponse {
        data: res.map_err(ServerError::from),
    })
}

#[async_trait]
impl SaplingRemoteApiHandler for CommitCloudUpdateArchive {
    type Request = UpdateArchiveParams;
    type Response = UpdateArchiveResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::CloudUpdateArchive;
    const ENDPOINT: &'static str = "/cloud/update_archive";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = update_archive(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn update_archive<R: MononokeRepo>(
    request: UpdateArchiveParams,
    repo: HgRepoContext<R>,
) -> anyhow::Result<UpdateArchiveResponse, Error> {
    Ok(UpdateArchiveResponse {
        data: repo
            .cloud_update_archive(&request.workspace, &request.reponame, request.archived)
            .await
            .map_err(ServerError::from),
    })
}

#[async_trait]
impl SaplingRemoteApiHandler for CommitCloudRenameWorkspace {
    type Request = RenameWorkspaceRequest;
    type Response = RenameWorkspaceResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::CloudRenameWorkspace;
    const ENDPOINT: &'static str = "/cloud/rename_workspace";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = rename_workspace(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn rename_workspace<R: MononokeRepo>(
    request: RenameWorkspaceRequest,
    repo: HgRepoContext<R>,
) -> anyhow::Result<RenameWorkspaceResponse, Error> {
    Ok(RenameWorkspaceResponse {
        data: repo
            .cloud_rename_workspace(
                &request.workspace,
                &request.reponame,
                &request.new_workspace,
            )
            .await
            .map_err(ServerError::from),
    })
}

#[async_trait]
impl SaplingRemoteApiHandler for CommitCloudSmartlogByVersion {
    type Request = GetSmartlogByVersionParams;
    type Response = SmartlogDataResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::CloudSmartlogByVersion;
    const ENDPOINT: &'static str = "/cloud/smartlog_by_version";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = get_smartlog_by_version(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn get_smartlog_by_version<R: MononokeRepo>(
    request: GetSmartlogByVersionParams,
    repo: HgRepoContext<R>,
) -> anyhow::Result<SmartlogDataResponse, Error> {
    let flags = request
        .flags
        .into_iter()
        .map(GetSmartlogFlag::into_cc_type)
        .collect::<anyhow::Result<Vec<_>>>()?;
    let filter = request.filter.into_cc_type()?;
    let cc_res = repo
        .cloud_smartlog_by_version(&request.workspace, &request.reponame, &filter, &flags)
        .await;
    let res = match cc_res {
        Ok(res) => Ok(SmartlogData::from_cc_type(res)?),
        Err(e) => Err(e),
    };
    Ok(SmartlogDataResponse {
        data: res.map_err(ServerError::from),
    })
}

#[async_trait]
impl SaplingRemoteApiHandler for CommitCloudHistoricalVersions {
    type Request = HistoricalVersionsParams;
    type Response = HistoricalVersionsResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::CloudHistoricalVersions;
    const ENDPOINT: &'static str = "/cloud/historical_versions";

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let res = historical_versions(request, repo).boxed();
        Ok(stream::once(res).boxed())
    }
}

async fn historical_versions<R: MononokeRepo>(
    request: HistoricalVersionsParams,
    repo: HgRepoContext<R>,
) -> anyhow::Result<HistoricalVersionsResponse, Error> {
    let cc_res = repo
        .cloud_historical_versions(&request.workspace, &request.reponame)
        .await;
    let res = match cc_res {
        Ok(res) => Ok(HistoricalVersionsData {
            versions: res
                .into_iter()
                .map(HistoricalVersion::from_cc_type)
                .collect::<anyhow::Result<Vec<HistoricalVersion>>>()?,
        }),
        Err(e) => Err(e),
    };

    Ok(HistoricalVersionsResponse {
        data: res.map_err(ServerError::from),
    })
}
