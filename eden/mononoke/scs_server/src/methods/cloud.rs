/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use mononoke_api_hg::RepoContextHgExt;

use crate::errors;
use crate::methods::thrift;
use crate::source_control_impl::SourceControlServiceImpl;

impl SourceControlServiceImpl {
    pub async fn cloud_workspace_info(
        &self,
        ctx: CoreContext,
        params: thrift::CloudWorkspaceInfoParams,
    ) -> Result<thrift::CloudWorkspaceInfoResponse, errors::ServiceError> {
        let repo = self.repo(ctx, &params.workspace.repo).await?;
        let info = repo
            .hg()
            .cloud_workspace(&params.workspace.name, &params.workspace.repo.name)
            .await
            .map_err(errors::invalid_request)?;

        Ok(thrift::CloudWorkspaceInfoResponse {
            workspace_info: thrift::WorkspaceInfo {
                specifier: thrift::WorkspaceSpecifier {
                    name: info.name,
                    repo: thrift::RepoSpecifier {
                        name: info.reponame,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                is_archived: info.archived,
                latest_version: info.version as i64,
                latest_timestamp: info.timestamp,
                ..Default::default()
            },
            ..Default::default()
        })
    }
}
