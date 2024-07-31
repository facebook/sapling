/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use commit_cloud::ctx::CommitCloudContext;
use commit_cloud::CommitCloudRef;
use edenapi_types::GetReferencesParams;
use edenapi_types::ReferencesData;
use edenapi_types::UpdateReferencesParams;
use edenapi_types::WorkspaceData;
use mononoke_api::MononokeError;

use crate::HgRepoContext;

impl HgRepoContext {
    pub async fn cloud_workspace(
        &self,
        workspace: &str,
        reponame: &str,
    ) -> Result<WorkspaceData, MononokeError> {
        let ctx = CommitCloudContext::new(workspace, reponame)?;
        let authz = self.repo().authorization_context();
        authz
            .require_commitcloud_operation(
                self.ctx(),
                &self.repo().repo(),
                workspace,
                reponame,
                "read",
            )
            .await?;
        Ok(self
            .repo()
            .inner_repo()
            .commit_cloud()
            .get_workspace(&ctx)
            .await?)
    }

    pub async fn cloud_workspaces(
        &self,
        prefix: &str,
        reponame: &str,
    ) -> Result<Vec<WorkspaceData>, MononokeError> {
        Ok(self
            .repo()
            .inner_repo()
            .commit_cloud()
            .get_workspaces(prefix, reponame)
            .await?)
    }

    pub async fn cloud_references(
        &self,
        params: &GetReferencesParams,
    ) -> Result<ReferencesData, MononokeError> {
        let authz = self.repo().authorization_context();
        authz
            .require_commitcloud_operation(
                self.ctx(),
                &self.repo().repo(),
                &params.workspace,
                &params.reponame,
                "read",
            )
            .await?;
        let ctx = CommitCloudContext::new(&params.workspace, &params.reponame)?;
        Ok(self
            .repo()
            .inner_repo()
            .commit_cloud()
            .get_references(&ctx, params)
            .await?)
    }

    pub async fn cloud_update_references(
        &self,
        params: &UpdateReferencesParams,
    ) -> Result<ReferencesData, MononokeError> {
        let ctx = CommitCloudContext::new(&params.workspace, &params.reponame)?;
        if params.version == 0 {
            ctx.check_workspace_name()?;
        }

        let authz = self.repo().authorization_context();
        authz
            .require_commitcloud_operation(
                self.ctx(),
                &self.repo().repo(),
                &params.workspace,
                &params.reponame,
                "write",
            )
            .await?;

        Ok(self
            .repo()
            .inner_repo()
            .commit_cloud()
            .update_references(&ctx, params)
            .await?)
    }
}
