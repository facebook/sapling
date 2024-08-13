/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]
pub mod ctx;
pub mod references;
pub mod sql;
use std::fmt::Display;
use std::sync::Arc;

use bonsai_hg_mapping::BonsaiHgMapping;
use changeset_info::ChangesetInfo;
#[cfg(fbcode_build)]
use commit_cloud_intern_utils::interngraph_publisher::publish_single_update;
#[cfg(fbcode_build)]
use commit_cloud_intern_utils::notification::NotificationData;
use context::CoreContext;
use edenapi_types::cloud::RemoteBookmark;
use edenapi_types::GetReferencesParams;
use edenapi_types::GetSmartlogParams;
use edenapi_types::HgId;
use edenapi_types::ReferencesData;
use edenapi_types::SmartlogNode;
use edenapi_types::UpdateReferencesParams;
use edenapi_types::WorkspaceData;
use facet::facet;
use mercurial_types::HgChangesetId;
use metaconfig_types::CommitCloudConfig;
use mononoke_types::Timestamp;
use permission_checker::AclProvider;
use permission_checker::BoxPermissionChecker;
use repo_derived_data::ArcRepoDerivedData;

use crate::ctx::CommitCloudContext;
use crate::references::cast_references_data;
use crate::references::fetch_references;
use crate::references::update_references_data;
use crate::references::versions::WorkspaceVersion;
use crate::references::RawSmartlogData;
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
    pub config: Arc<CommitCloudConfig>,
}

impl CommitCloud {
    pub fn new(
        storage: SqlCommitCloud,
        bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
        repo_derived_data: ArcRepoDerivedData,
        core_ctx: CoreContext,
        acl_provider: Arc<dyn AclProvider>,
        config: Arc<CommitCloudConfig>,
    ) -> Self {
        CommitCloud {
            storage,
            bonsai_hg_mapping,
            repo_derived_data,
            core_ctx,
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
    pub async fn get_workspace(&self, ctx: &CommitCloudContext) -> anyhow::Result<WorkspaceData> {
        let maybeworkspace =
            WorkspaceVersion::fetch_from_db(&self.storage, &ctx.workspace, &ctx.reponame).await?;
        if let Some(res) = maybeworkspace {
            return Ok(res.into_workspace_data(&ctx.reponame));
        }
        Err(anyhow::anyhow!(
            "'get_workspace' failed: workspace {} does not exist",
            ctx.workspace
        ))
    }

    pub async fn get_workspaces(
        &self,
        prefix: &str,
        reponame: &str,
    ) -> anyhow::Result<Vec<WorkspaceData>> {
        if reponame.is_empty() || prefix.is_empty() {
            return Err(anyhow::anyhow!(
                "'get workspaces' failed: empty reponame or prefix "
            ));
        }

        if prefix.len() < 3 {
            return Err(anyhow::anyhow!(
                "'get workspaces' failed: prefix must be at least 3 characters "
            ));
        }
        let maybeworkspace =
            WorkspaceVersion::fetch_by_prefix(&self.storage, prefix, reponame).await?;

        Ok(maybeworkspace
            .into_iter()
            .map(|wp| wp.into_workspace_data(reponame))
            .collect())
    }

    pub async fn get_references(
        &self,
        ctx: &CommitCloudContext,
        params: &GetReferencesParams,
    ) -> anyhow::Result<ReferencesData> {
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
                "'get_references' failed: workspace {} has been removed or renamed",
                ctx.workspace.clone()
            ));
        }

        if base_version > latest_version {
            return Err(anyhow::anyhow!(
                "'get_references' failed: base version {} is greater than latest version {}",
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

        let raw_references_data = fetch_references(ctx, &self.storage).await?;

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
        ctx: &CommitCloudContext,
        params: &UpdateReferencesParams,
    ) -> anyhow::Result<ReferencesData> {
        let mut latest_version: u64 = 0;
        let mut version_timestamp: i64 = 0;

        let maybeworkspace =
            WorkspaceVersion::fetch_from_db(&self.storage, &ctx.workspace, &ctx.reponame).await?;

        if let Some(workspace_version) = maybeworkspace {
            latest_version = workspace_version.version;
            version_timestamp = workspace_version.timestamp.timestamp_nanos();
        }

        if params.version < latest_version {
            let raw_references_data = fetch_references(ctx, &self.storage).await?;
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
            txn = update_references_data(&self.storage, txn, cri, params.clone(), ctx).await?;
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

    pub async fn commit_cloud_acl(
        &self,
        name: &str,
    ) -> anyhow::Result<Option<BoxPermissionChecker>> {
        self.acl_provider
            .commitcloud_workspace_acl(name, &None)
            .await
    }

    pub async fn get_smartlog_raw_info(
        &self,
        params: &GetSmartlogParams,
    ) -> anyhow::Result<RawSmartlogData> {
        RawSmartlogData::fetch_smartlog_references(
            &CommitCloudContext::new(&params.workspace, &params.reponame)?,
            &self.storage,
            &params.flags,
        )
        .await
    }

    pub fn make_smartlog_node(
        &self,
        hgid: &HgChangesetId,
        parents: &Vec<HgId>,
        node: &ChangesetInfo,
        local_bookmarks: &Option<Vec<String>>,
        remote_bookmarks: &Option<Vec<RemoteBookmark>>,
        phase: &Phase,
    ) -> anyhow::Result<SmartlogNode> {
        let author = node.author();
        let date = node.author_date().as_chrono().timestamp();
        let message = node.message();

        let node = SmartlogNode {
            node: (*hgid).into(),
            phase: phase.to_string(),
            author: author.to_string(),
            date,
            message: message.to_string(),
            parents: parents.to_owned(),
            bookmarks: local_bookmarks.to_owned().unwrap_or_default(),
            remote_bookmarks: remote_bookmarks.to_owned(),
        };
        Ok(node)
    }
}
