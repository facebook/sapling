/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use commit_cloud_helpers::sanity_check_workspace_name;
use commit_cloud_types::SmartlogFlag;
use commit_cloud_types::SmartlogNode;
use commit_cloud_types::WorkspaceData;
use commit_cloud_types::WorkspaceRemoteBookmark as CCWorkspaceRemoteBookmark;
use context::CoreContext;
use mononoke_api_hg::RepoContextHgExt;

use crate::errors::invalid_request;
use crate::errors::ServiceError;
use crate::into_response::IntoResponse;
use crate::methods::thrift;
use crate::source_control_impl::SourceControlServiceImpl;

const USER_WORKSPACE_PREFIX: &str = "user/";

impl SourceControlServiceImpl {
    pub async fn cloud_workspace_info(
        &self,
        ctx: CoreContext,
        params: thrift::CloudWorkspaceInfoParams,
    ) -> Result<thrift::CloudWorkspaceInfoResponse, ServiceError> {
        let repo = self.repo(ctx, &params.workspace.repo).await?;
        let info = repo
            .hg()
            .cloud_workspace(&params.workspace.name, &params.workspace.repo.name)
            .await
            .map_err(invalid_request)?;

        Ok(thrift::CloudWorkspaceInfoResponse {
            workspace_info: info.into_response(),
            ..Default::default()
        })
    }

    pub async fn cloud_user_workspaces(
        &self,
        ctx: CoreContext,
        params: thrift::CloudUserWorkspacesParams,
    ) -> Result<thrift::CloudUserWorkspacesResponse, ServiceError> {
        if !commit_cloud_helpers::is_valid_linux_user(&params.user) {
            return Err(ServiceError::Request(invalid_request(format!(
                "{} is not a valid unixname",
                &params.user
            ))));
        }

        let repo = self.repo(ctx, &params.repo).await?;
        let prefix = format!("{}{}/", USER_WORKSPACE_PREFIX, &params.user);
        let info = repo
            .hg()
            .cloud_workspaces(&prefix, &params.repo.name)
            .await
            .map_err(invalid_request)?;

        let workspaces = info.into_iter().map(WorkspaceData::into_response).collect();

        Ok(thrift::CloudUserWorkspacesResponse {
            workspaces,
            ..Default::default()
        })
    }

    pub async fn cloud_workspace_smartlog(
        &self,
        ctx: CoreContext,
        params: thrift::CloudWorkspaceSmartlogParams,
    ) -> Result<thrift::CloudWorkspaceSmartlogResponse, ServiceError> {
        if !sanity_check_workspace_name(&params.workspace.name) {
            return Err(ServiceError::Request(invalid_request(format!(
                "Invalid workspace name: {}",
                &params.workspace.name
            ))));
        }
        let repo = self.repo(ctx, &params.workspace.repo).await?;
        repo.clone()
            .hg()
            .cloud_workspace(&params.workspace.name, &params.workspace.repo.name)
            .await
            .map_err(invalid_request)?;

        let flags = params
            .flags
            .into_iter()
            .map(from_thrift_smartlog_flag)
            .collect::<Result<Vec<_>>>()
            .map_err(|e| ServiceError::Request(invalid_request(e.to_string())))?;
        let smartlog = repo
            .hg()
            .cloud_smartlog(&params.workspace.name, &params.workspace.repo.name, &flags)
            .await?;

        let nodes = smartlog
            .nodes
            .into_iter()
            .map(into_thrift_smartlog_node)
            .collect();

        Ok(thrift::CloudWorkspaceSmartlogResponse {
            smartlog: thrift::SmartlogData {
                nodes,
                version: smartlog.version,
                timestamp: smartlog.timestamp,
                ..Default::default()
            },
            ..Default::default()
        })
    }
}

fn from_thrift_smartlog_flag(t: thrift::CloudWorkspaceSmartlogFlags) -> Result<SmartlogFlag> {
    match t {
        thrift::CloudWorkspaceSmartlogFlags::SKIP_PUBLIC_COMMITS_METADATA => {
            Ok(SmartlogFlag::SkipPublicCommitsMetadata)
        }
        thrift::CloudWorkspaceSmartlogFlags::ADD_REMOTE_BOOKMARKS => {
            Ok(SmartlogFlag::AddRemoteBookmarks)
        }
        thrift::CloudWorkspaceSmartlogFlags::ADD_ALL_BOOKMARKS => Ok(SmartlogFlag::AddAllBookmarks),
        _ => Err(anyhow::anyhow!("Invalid smartlog flag")),
    }
}

fn into_thrift_smartlog_node(node: SmartlogNode) -> thrift::SmartlogNode {
    thrift::SmartlogNode {
        hg_id: node.node.to_string(),
        phase: node.phase,
        author: node.author,
        date: node.date,
        message: node.message,
        parents: node.parents.into_iter().map(|p| p.to_string()).collect(),
        bookmarks: node.bookmarks,
        remote_bookmarks: node.remote_bookmarks.map(|rbs| {
            rbs.into_iter()
                .map(into_thrift_remote_bookmark)
                .collect::<Vec<thrift::WorkspaceRemoteBookmark>>()
        }),
        ..Default::default()
    }
}

fn into_thrift_remote_bookmark(b: CCWorkspaceRemoteBookmark) -> thrift::WorkspaceRemoteBookmark {
    thrift::WorkspaceRemoteBookmark {
        remote: b.remote().to_string(),
        name: b.name().to_string(),
        hg_id: Some(b.commit().to_string()),
        ..Default::default()
    }
}
