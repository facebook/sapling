/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]
pub mod references;
pub mod sql;
pub mod workspace;
use std::sync::Arc;

use bonsai_hg_mapping::BonsaiHgMapping;
use context::CoreContext;
use edenapi_types::GetReferencesParams;
use edenapi_types::ReferencesData;
use edenapi_types::UpdateReferencesParams;
use facet::facet;
use mononoke_types::Timestamp;
use references::update_references_data;
use repo_derived_data::ArcRepoDerivedData;

use crate::references::cast_references_data;
use crate::references::fetch_references;
use crate::sql::ops::Get;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::versions_ops::WorkspaceVersion;

#[facet]
pub struct CommitCloud {
    pub storage: SqlCommitCloud,
    pub bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
    pub repo_derived_data: ArcRepoDerivedData,
    pub core_ctx: CoreContext,
}

#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub hostname: String,
    pub reporoot: String,
    pub version: u64,
}

#[derive(Debug, Clone)]
pub struct CommitCloudContext {
    pub reponame: String,
    pub workspace: String,
}

impl CommitCloud {
    pub async fn get_workspace(
        &self,
        workspace: &str,
        reponame: &str,
    ) -> anyhow::Result<Vec<WorkspaceVersion>> {
        let workspace: anyhow::Result<Vec<WorkspaceVersion>> = self
            .storage
            .get(reponame.to_owned(), workspace.to_owned())
            .await;
        workspace
    }

    pub async fn get_references(
        &self,
        params: GetReferencesParams,
    ) -> anyhow::Result<ReferencesData> {
        let ctx = CommitCloudContext {
            workspace: params.workspace.clone(),
            reponame: params.reponame.clone(),
        };

        let base_version = params.version;

        let mut latest_version: u64 = 0;
        let mut version_timestamp: i64 = 0;
        let maybeworkspace = self
            .get_workspace(&ctx.workspace.clone(), &ctx.reponame.clone())
            .await?;
        if !maybeworkspace.is_empty() {
            latest_version = maybeworkspace[0].version;
            version_timestamp = maybeworkspace[0].timestamp.timestamp_nanos();
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

        let raw_references_data = fetch_references(ctx.clone(), &self.storage).await?;

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
        params: UpdateReferencesParams,
    ) -> anyhow::Result<ReferencesData> {
        let ctx = CommitCloudContext {
            workspace: params.workspace.clone(),
            reponame: params.reponame.clone(),
        };
        let mut latest_version: u64 = 0;
        let mut version_timestamp: i64 = 0;

        let maybeworkspace = self
            .get_workspace(&ctx.workspace.clone(), &ctx.reponame.clone())
            .await?;

        if !maybeworkspace.is_empty() {
            latest_version = maybeworkspace[0].version;
            version_timestamp = maybeworkspace[0].timestamp.timestamp_nanos();
        }
        let new_version = latest_version + 1;
        if params.version < latest_version {
            let raw_references_data = fetch_references(ctx.clone(), &self.storage).await?;
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

        update_references_data(&self.storage, params, &ctx).await?;
        let new_version_timestamp = Timestamp::now();
        let args = WorkspaceVersion {
            workspace: ctx.workspace.clone(),
            version: new_version,
            timestamp: new_version_timestamp,
            archived: false,
        };
        let _ = &self
            .storage
            .insert(ctx.reponame.clone(), ctx.workspace.clone(), args.clone())
            .await?;

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
}
