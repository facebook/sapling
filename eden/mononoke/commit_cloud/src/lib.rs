/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]
pub mod ctx;
pub mod references;
pub mod smartlog;
pub mod sql;
use std::fmt::Display;
use std::sync::Arc;

use anyhow::bail;
use anyhow::ensure;
use bonsai_hg_mapping::BonsaiHgMapping;
use commit_cloud_helpers::make_workspace_acl_name;
#[cfg(fbcode_build)]
use commit_cloud_intern_utils::acl_check::ACL_LINK;
#[cfg(fbcode_build)]
use commit_cloud_intern_utils::interngraph_publisher::publish_single_update;
#[cfg(fbcode_build)]
use commit_cloud_intern_utils::notification::NotificationData;
use context::CoreContext;
use edenapi_types::cloud::WorkspaceSharingData;
use edenapi_types::GetReferencesParams;
use edenapi_types::ReferencesData;
use edenapi_types::UpdateReferencesParams;
use edenapi_types::WorkspaceData;
use facet::facet;
use futures_stats::futures03::TimedFutureExt;
use metaconfig_types::CommitCloudConfig;
use mononoke_types::Timestamp;
use permission_checker::AclProvider;
use permission_checker::BoxPermissionChecker;
use references::history::WorkspaceHistory;
use references::rename_all;
use repo_derived_data::ArcRepoDerivedData;
use sql::versions_ops::UpdateVersionArgs;

use crate::ctx::CommitCloudContext;
use crate::references::cast_references_data;
use crate::references::fetch_references;
use crate::references::update_references_data;
use crate::references::versions::WorkspaceVersion;
use crate::sql::ops::Get;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;

#[facet]
pub struct CommitCloud {
    pub storage: SqlCommitCloud,
    pub bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
    pub repo_derived_data: ArcRepoDerivedData,
    pub ctx: CoreContext,
    pub acl_provider: Arc<dyn AclProvider>,
    pub config: Arc<CommitCloudConfig>,
}

impl CommitCloud {
    pub fn new(
        storage: SqlCommitCloud,
        bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
        repo_derived_data: ArcRepoDerivedData,
        ctx: CoreContext,
        acl_provider: Arc<dyn AclProvider>,
        config: Arc<CommitCloudConfig>,
    ) -> Self {
        CommitCloud {
            storage,
            bonsai_hg_mapping,
            repo_derived_data,
            ctx,
            acl_provider,
            config,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub hostname: String,
    pub reporoot: String,
    pub version: u64,
}

pub enum Phase {
    Public,
    Draft,
}

impl Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Phase::Public => write!(f, "public"),
            Phase::Draft => write!(f, "draft"),
        }
    }
}

impl CommitCloud {
    pub async fn get_workspace(
        &self,
        cc_ctx: &CommitCloudContext,
    ) -> anyhow::Result<WorkspaceData> {
        let maybeworkspace =
            WorkspaceVersion::fetch_from_db(&self.storage, &cc_ctx.workspace, &cc_ctx.reponame)
                .await?;
        if let Some(res) = maybeworkspace {
            return Ok(res.into_workspace_data(&cc_ctx.reponame));
        }
        Err(anyhow::anyhow!(
            "'get_workspace' failed: workspace {} does not exist",
            cc_ctx.workspace
        ))
    }

    pub async fn get_workspaces(
        &self,
        prefix: &str,
        reponame: &str,
    ) -> anyhow::Result<Vec<WorkspaceData>> {
        ensure!(
            !reponame.is_empty() && !prefix.is_empty(),
            "'get workspaces' failed: empty reponame or prefix "
        );

        ensure!(
            prefix.len() >= 3,
            "'get workspaces' failed: prefix must be at least 3 characters "
        );

        let maybeworkspace =
            WorkspaceVersion::fetch_by_prefix(&self.storage, prefix, reponame).await?;

        Ok(maybeworkspace
            .into_iter()
            .map(|wp| wp.into_workspace_data(reponame))
            .collect())
    }

    pub async fn get_references(
        &self,
        cc_ctx: &CommitCloudContext,
        params: &GetReferencesParams,
    ) -> anyhow::Result<ReferencesData> {
        let base_version = params.version;

        let mut latest_version: u64 = 0;
        let mut version_timestamp: i64 = 0;
        let maybeworkspace =
            WorkspaceVersion::fetch_from_db(&self.storage, &cc_ctx.workspace, &cc_ctx.reponame)
                .await?;
        if let Some(workspace_version) = maybeworkspace {
            latest_version = workspace_version.version;
            version_timestamp = workspace_version.timestamp.timestamp_nanos();
        }

        ensure!(
            base_version <= latest_version,
            if latest_version == 0 {
                format!(
                    "'get_references' failed: workspace {} has been removed or renamed",
                    cc_ctx.workspace.clone()
                )
            } else {
                format!(
                    "'get_references' failed: base version {} is greater than latest version {}",
                    base_version, latest_version
                )
            }
        );

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

        let raw_references_data = fetch_references(cc_ctx, &self.storage).await?;

        let references_data = cast_references_data(
            raw_references_data,
            latest_version,
            version_timestamp,
            self.bonsai_hg_mapping.clone(),
            self.repo_derived_data.clone(),
            &self.ctx,
        )
        .await?;

        Ok(references_data)
    }

    pub async fn update_references(
        &self,
        cc_ctx: &CommitCloudContext,
        params: &UpdateReferencesParams,
    ) -> anyhow::Result<ReferencesData> {
        let mut latest_version: u64 = 0;
        let mut version_timestamp: i64 = 0;

        let maybeworkspace =
            WorkspaceVersion::fetch_from_db(&self.storage, &cc_ctx.workspace, &cc_ctx.reponame)
                .await?;

        if let Some(workspace_version) = maybeworkspace {
            latest_version = workspace_version.version;
            version_timestamp = workspace_version.timestamp.timestamp_nanos();
        }
        let raw_references_data = fetch_references(cc_ctx, &self.storage).await?;
        if params.version < latest_version {
            return cast_references_data(
                raw_references_data,
                latest_version,
                version_timestamp,
                self.bonsai_hg_mapping.clone(),
                self.repo_derived_data.clone(),
                &self.ctx,
            )
            .await;
        }

        let mut txn = self
            .storage
            .connections
            .write_connection
            .start_transaction()
            .await?;
        let cri = self.ctx.client_request_info();

        let initiate_workspace = params.version == 0
            && (params.new_heads.is_empty()
                && params.updated_bookmarks.is_empty()
                && !params
                    .updated_remote_bookmarks
                    .clone()
                    .is_some_and(|x| !x.is_empty()));

        if !initiate_workspace {
            txn = update_references_data(&self.storage, txn, cri, params.clone(), cc_ctx).await?;
        }

        let new_version_timestamp = Timestamp::now();
        let new_version = latest_version + 1;

        let args = WorkspaceVersion {
            workspace: cc_ctx.workspace.clone(),
            version: new_version,
            timestamp: new_version_timestamp,
            archived: false,
        };

        txn = self
            .storage
            .insert(
                txn,
                cri,
                cc_ctx.reponame.clone(),
                cc_ctx.workspace.clone(),
                args.clone(),
            )
            .await?;

        let history_entry = WorkspaceHistory::from_references(
            raw_references_data,
            latest_version,
            version_timestamp,
        );

        txn = self
            .storage
            .insert(
                txn,
                cri,
                cc_ctx.reponame.clone(),
                cc_ctx.workspace.clone(),
                history_entry,
            )
            .await?;

        txn.commit().await?;

        #[cfg(fbcode_build)]
        if !self.config.disable_interngraph_notification && !initiate_workspace {
            let notification =
                NotificationData::from_update_references_params(params.clone(), new_version);
            let (stats, res) = publish_single_update(
                notification,
                &cc_ctx.workspace.clone(),
                &cc_ctx.reponame.clone(),
                self.ctx.fb,
            )
            .timed()
            .await;
            self.ctx
                .scuba()
                .clone()
                .add_future_stats(&stats)
                .log_with_msg(
                    "commit cloud: sent interngraph notification",
                    format!(
                        "For workspace {} in repo {} with response {}",
                        cc_ctx.workspace, cc_ctx.reponame, res?
                    ),
                );
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

    pub async fn commit_cloud_acl(
        &self,
        name: &str,
    ) -> anyhow::Result<Option<BoxPermissionChecker>> {
        self.acl_provider
            .commitcloud_workspace_acl(name, &None)
            .await
    }

    pub async fn share_workspace(
        &self,
        ctx: &CommitCloudContext,
    ) -> anyhow::Result<WorkspaceSharingData> {
        let maybeworkspace =
            WorkspaceVersion::fetch_from_db(&self.storage, &ctx.workspace, &ctx.reponame).await?;
        if maybeworkspace.is_none() {
            bail!(
                "'share_workspace' failed: workspace {} does not exist in repo {}",
                ctx.workspace,
                ctx.reponame
            )
        }

        if maybeworkspace.unwrap().archived {
            bail!(
                "'share_workspace' failed: workspace {} has been archived",
                ctx.workspace
            );
        }
        let acl_name = make_workspace_acl_name(&ctx.workspace, &ctx.reponame);

        #[cfg(fbcode_build)]
        let link = format!("[{}{}]", ACL_LINK, acl_name);
        #[cfg(not(fbcode_build))]
        let link = String::new();

        let maybe_acl = self.commit_cloud_acl(&acl_name).await?;
        if maybe_acl.is_some() {
            return Ok(WorkspaceSharingData {
                acl_name: acl_name.clone(),
                sharing_message: format!(
                    "'share_workspace' succeeded: workspace {} has been already shared under the acl {} {}",
                    ctx.workspace, &acl_name, &link
                ),
            });
        }

        if ctx.owner.is_none() {
            bail!(
                "'share_workspace' failed: no owner inferred for workspace {} ",
                ctx.workspace
            );
        }

        match self
            .acl_provider
            .commitcloud_workspace_acl(&acl_name, &ctx.owner)
            .await
        {
            Err(e) => bail!(
                "'share_workspace' failed: unable to create acl {} for workspace {}: {} ",
                ctx.workspace,
                acl_name,
                e
            ),
            Ok(_) => Ok(WorkspaceSharingData {
                acl_name: acl_name.clone(),
                sharing_message: format!(
                    "'share_workspace' succeeded: workspace {} is now marked for sharing through the ACL {} [{}]",
                    ctx.workspace, &acl_name, &link
                ),
            }),
        }
    }

    pub async fn rename_workspace(
        &self,
        cc_ctx: &CommitCloudContext,
        new_workspace: &str,
    ) -> anyhow::Result<String> {
        ensure!(
            !new_workspace.is_empty(),
            "'rename_workspace' failed: new workspace name cannot be empty"
        );

        ensure!(
            WorkspaceVersion::fetch_from_db(&self.storage, new_workspace, &cc_ctx.reponame)
                .await?
                .is_none(),
            format!(
                "'rename_workspace' failed: workspace {} already exists",
                new_workspace
            ),
        );

        let cri = self.ctx.client_request_info();
        let (txn, affected_rows) = rename_all(&self.storage, cri, &cc_ctx, new_workspace).await?;

        ensure!(
            affected_rows > 0,
            format!(
                "'rename_workspace' failed: workspace {} does not exist",
                cc_ctx.workspace
            )
        );
        txn.commit().await?;

        Ok("'rename_workspace' succeeded".to_string())
    }

    pub async fn update_workspace_archive(
        &self,
        cc_ctx: &CommitCloudContext,
        archived: bool,
    ) -> anyhow::Result<String> {
        // Check if workspace exists
        let _ = self.get_workspace(cc_ctx).await?;

        let txn = self
            .storage
            .connections
            .write_connection
            .start_transaction()
            .await?;
        let cri = self.ctx.client_request_info();
        let (txn, affected_rows) = Update::<WorkspaceVersion>::update(
            &self.storage,
            txn,
            cri,
            cc_ctx.clone(),
            UpdateVersionArgs::Archive(archived),
        )
        .await?;
        txn.commit().await?;

        ensure!(
            affected_rows > 0,
            "'update_workspace_archive' failed: failed on updating the workspace {} from DB",
            cc_ctx.workspace.clone()
        );

        Ok(String::from("'update_workspace_archive' succeeded"))
    }
}
