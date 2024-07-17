/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]
pub mod references;
pub mod sql;

use std::sync::Arc;

use bonsai_hg_mapping::BonsaiHgMapping;
use commit_cloud_helpers::sanity_check_workspace_name;
#[cfg(fbcode_build)]
use commit_cloud_intern_utils::interngraph_publisher::publish_single_update;
#[cfg(fbcode_build)]
use commit_cloud_intern_utils::notification::NotificationData;
use context::CoreContext;
use edenapi_types::cloud::SmartlogData;
use edenapi_types::GetReferencesParams;
use edenapi_types::GetSmartlogParams;
use edenapi_types::ReferencesData;
use edenapi_types::UpdateReferencesParams;
use edenapi_types::WorkspaceData;
use facet::facet;
use mononoke_types::Timestamp;
use permission_checker::AclProvider;
use permission_checker::DefaultAclProvider;
use references::update_references_data;
use repo_derived_data::ArcRepoDerivedData;

use crate::references::cast_references_data;
use crate::references::fetch_references;
use crate::references::versions::WorkspaceVersion;
use crate::sql::ops::Get;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;

#[facet]
pub struct CommitCloud {
    pub storage: SqlCommitCloud,
    pub bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
    pub repo_derived_data: ArcRepoDerivedData,
    pub core_ctx: CoreContext,
    pub acl_provider: Arc<dyn AclProvider>,
}

impl CommitCloud {
    pub fn new(
        storage: SqlCommitCloud,
        bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
        repo_derived_data: ArcRepoDerivedData,
        core_ctx: CoreContext,
    ) -> Self {
        let acl_provider = DefaultAclProvider::new(core_ctx.fb);
        CommitCloud {
            storage,
            bonsai_hg_mapping,
            repo_derived_data,
            core_ctx,
            acl_provider,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub hostname: String,
    pub reporoot: String,
    pub version: u64,
}

#[derive(Debug, Clone)]
pub struct CommitCloudContext {
    pub workspace: String,
    pub reponame: String,
}

impl CommitCloudContext {
    pub fn new(workspace: &str, reponame: &str) -> Self {
        Self {
            workspace: workspace.to_owned(),
            reponame: reponame.to_owned(),
        }
    }
}

impl CommitCloud {
    pub async fn get_workspace(
        &self,
        workspace: &str,
        reponame: &str,
    ) -> anyhow::Result<WorkspaceData> {
        if workspace.is_empty() || reponame.is_empty() {
            return Err(anyhow::anyhow!(
                "'get_workspace' failed: empty repo_name or workspace"
            ));
        }

        let maybeworkspace =
            WorkspaceVersion::fetch_from_db(&self.storage, workspace, reponame).await?;
        if let Some(res) = maybeworkspace {
            return Ok(res.into_workspace_data(reponame));
        }
        Err(anyhow::anyhow!("Workspace {} does not exist", workspace))
    }

    pub async fn get_references(
        &self,
        params: &GetReferencesParams,
    ) -> anyhow::Result<ReferencesData> {
        let ctx = CommitCloudContext::new(&params.workspace, &params.reponame);

        let base_version = params.version;

        let mut latest_version: u64 = 0;
        let mut version_timestamp: i64 = 0;
        let maybeworkspace =
            WorkspaceVersion::fetch_from_db(&self.storage, &ctx.workspace, &ctx.reponame).await?;
        if let Some(workspace_version) = maybeworkspace {
            latest_version = workspace_version.version;
            version_timestamp = workspace_version.timestamp.timestamp_nanos();
        }
        if base_version > latest_version && latest_version == 0 {
            return Err(anyhow::anyhow!(
                "Workspace {} has been removed or renamed",
                ctx.workspace.clone()
            ));
        }

        if base_version > latest_version {
            return Err(anyhow::anyhow!(
                "Base version {} is greater than latest version {}",
                base_version,
                latest_version
            ));
        }

        if base_version == latest_version {
            return Ok(ReferencesData {
                version: latest_version,
                heads: None,
                bookmarks: None,
                heads_dates: None,
                remote_bookmarks: None,
                snapshots: None,
                timestamp: Some(version_timestamp),
            });
        }

        let raw_references_data = fetch_references(&ctx, &self.storage).await?;

        let references_data = cast_references_data(
            raw_references_data,
            latest_version,
            version_timestamp,
            self.bonsai_hg_mapping.clone(),
            self.repo_derived_data.clone(),
            &self.core_ctx,
        )
        .await?;

        Ok(references_data)
    }

    pub async fn update_references(
        &self,
        params: &UpdateReferencesParams,
    ) -> anyhow::Result<ReferencesData> {
        if params.workspace.is_empty() || params.reponame.is_empty() {
            return Err(anyhow::anyhow!(
                "'update_references' failed: empty repo_name or workspace"
            ));
        }

        if params.version == 0 && !sanity_check_workspace_name(&params.workspace) {
            return Err(anyhow::anyhow!(
                "'update_references' failed: creating a new workspace with name '{}' is not allowed",
                params.workspace
            ));
        }

        let ctx = CommitCloudContext::new(&params.workspace, &params.reponame);
        let mut latest_version: u64 = 0;
        let mut version_timestamp: i64 = 0;

        let maybeworkspace =
            WorkspaceVersion::fetch_from_db(&self.storage, &ctx.workspace, &ctx.reponame).await?;

        if let Some(workspace_version) = maybeworkspace {
            latest_version = workspace_version.version;
            version_timestamp = workspace_version.timestamp.timestamp_nanos();
        }

        if params.version < latest_version {
            let raw_references_data = fetch_references(&ctx, &self.storage).await?;
            return cast_references_data(
                raw_references_data,
                latest_version,
                version_timestamp,
                self.bonsai_hg_mapping.clone(),
                self.repo_derived_data.clone(),
                &self.core_ctx,
            )
            .await;
        }

        let mut txn = self
            .storage
            .connections
            .write_connection
            .start_transaction()
            .await?;
        let cri = self.core_ctx.client_request_info();

        let initiate_workspace = params.version == 0
            && (params.new_heads.is_empty()
                && params.updated_bookmarks.is_empty()
                && !params
                    .updated_remote_bookmarks
                    .clone()
                    .is_some_and(|x| !x.is_empty()));

        if !initiate_workspace {
            txn = update_references_data(&self.storage, txn, cri, params.clone(), &ctx).await?;
        }

        let new_version_timestamp = Timestamp::now();
        let new_version = latest_version + 1;

        let args = WorkspaceVersion {
            workspace: ctx.workspace.clone(),
            version: new_version,
            timestamp: new_version_timestamp,
            archived: false,
        };

        txn = self
            .storage
            .insert(
                txn,
                cri,
                ctx.reponame.clone(),
                ctx.workspace.clone(),
                args.clone(),
            )
            .await?;

        txn.commit().await?;

        #[cfg(fbcode_build)]
        if std::env::var_os("SANDCASTLE").map_or(true, |sc| sc != "1") && !initiate_workspace {
            let notification =
                NotificationData::from_update_references_params(params.clone(), new_version);
            let _ = publish_single_update(
                notification,
                &ctx.workspace.clone(),
                &ctx.reponame.clone(),
                self.core_ctx.fb,
            )
            .await?;
        }
        Ok(ReferencesData {
            version: new_version,
            heads: None,
            bookmarks: None,
            heads_dates: None,
            remote_bookmarks: None,
            snapshots: None,
            timestamp: Some(new_version_timestamp.timestamp_nanos()),
        })
    }

    pub async fn get_smartlog(&self, params: &GetSmartlogParams) -> anyhow::Result<SmartlogData> {
        if params.workspace.is_empty() || params.reponame.is_empty() {
            return Err(anyhow::anyhow!(
                "'get_smartlog' failed: empty repo_name or workspace"
            ));
        }
        Err(anyhow::anyhow!("Not implemented"))
    }
}
