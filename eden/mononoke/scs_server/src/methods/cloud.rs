/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use mononoke_api_hg::RepoContextHgExt;
use mononoke_types::commit_cloud::WorkspaceData;

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
        let prefix = format!("{}{}", USER_WORKSPACE_PREFIX, &params.user);
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
        _ctx: CoreContext,
        _params: thrift::CloudWorkspaceSmartlogParams,
    ) -> Result<thrift::CloudWorkspaceSmartlogResponse, ServiceError> {
        Err(ServiceError::Request(invalid_request(
            "'cloud_workspace_smartlog' is not implemented yet".to_string(),
        )))
    }
}
