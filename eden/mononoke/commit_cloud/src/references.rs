/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use bonsai_hg_mapping::BonsaiHgMapping;
use changeset_info::ChangesetInfo;
use context::CoreContext;
use edenapi_types::cloud::RemoteBookmark;
use edenapi_types::HgId;
use edenapi_types::ReferencesData;
use edenapi_types::UpdateReferencesParams;
use repo_derived_data::ArcRepoDerivedData;

use crate::references::heads::update_heads;
use crate::references::heads::WorkspaceHead;
use crate::references::local_bookmarks::update_bookmarks;
use crate::references::local_bookmarks::WorkspaceLocalBookmark;
use crate::references::remote_bookmarks::update_remote_bookmarks;
use crate::references::remote_bookmarks::WorkspaceRemoteBookmark;
use crate::references::snapshots::update_snapshots;
use crate::references::snapshots::WorkspaceSnapshot;
use crate::sql::ops::Get;
use crate::sql::ops::SqlCommitCloud;
use crate::CommitCloudContext;

pub mod heads;
pub mod history;
pub mod local_bookmarks;
pub mod remote_bookmarks;
pub mod snapshots;
// Workspace information as we retrieve it form the database
#[derive(Debug, Clone)]
pub struct RawReferencesData {
    pub heads: Vec<WorkspaceHead>,
    pub local_bookmarks: Vec<WorkspaceLocalBookmark>,
    pub remote_bookmarks: Vec<WorkspaceRemoteBookmark>,
    pub snapshots: Vec<WorkspaceSnapshot>,
}

// Perform all get queries into the database
pub(crate) async fn fetch_references(
    ctx: CommitCloudContext,
    sql: &SqlCommitCloud,
) -> Result<RawReferencesData, anyhow::Error> {
    let heads: Vec<WorkspaceHead> = sql.get(ctx.reponame.clone(), ctx.workspace.clone()).await?;

    let local_bookmarks: Vec<WorkspaceLocalBookmark> =
        sql.get(ctx.reponame.clone(), ctx.workspace.clone()).await?;

    let remote_bookmarks: Vec<WorkspaceRemoteBookmark> =
        sql.get(ctx.reponame.clone(), ctx.workspace.clone()).await?;

    let snapshots: Vec<WorkspaceSnapshot> =
        sql.get(ctx.reponame.clone(), ctx.workspace.clone()).await?;

    Ok(RawReferencesData {
        heads,
        local_bookmarks,
        remote_bookmarks,
        snapshots,
    })
}

// Cast the raw data into the format the client expects it
pub(crate) async fn cast_references_data(
    raw_references_data: RawReferencesData,
    latest_version: u64,
    version_timestamp: i64,
    bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
    repo_derived_data: ArcRepoDerivedData,
    core_ctx: &CoreContext,
) -> Result<ReferencesData, anyhow::Error> {
    let mut heads: Vec<HgId> = Vec::new();
    let mut bookmarks: HashMap<String, HgId> = HashMap::new();
    let mut heads_dates: HashMap<HgId, i64> = HashMap::new();
    let mut remote_bookmarks: Vec<RemoteBookmark> = Vec::new();
    let mut snapshots: Vec<HgId> = Vec::new();

    for head in raw_references_data.heads {
        heads.push(head.commit.into());
        let bonsai = bonsai_hg_mapping
            .get_bonsai_from_hg(core_ctx, head.commit)
            .await?;
        match bonsai {
            Some(bonsai) => {
                let cs_info = repo_derived_data
                    .derive::<ChangesetInfo>(core_ctx, bonsai.clone())
                    .await?;
                let cs_date = cs_info.author_date();
                heads_dates.insert(head.commit.into(), cs_date.as_chrono().timestamp());
            }
            None => {
                return Err(anyhow!(
                    "Changeset {} not found in bonsai mapping",
                    head.commit
                ));
            }
        }
    }
    for bookmark in raw_references_data.local_bookmarks {
        bookmarks.insert(bookmark.name.clone(), bookmark.commit.into());
    }

    for remote_bookmark in raw_references_data.remote_bookmarks {
        remote_bookmarks.push(RemoteBookmark {
            remote: remote_bookmark.remote.clone(),
            name: remote_bookmark.name.clone(),
            node: Some(remote_bookmark.commit.into()),
        });
    }

    for snapshot in raw_references_data.snapshots {
        snapshots.push(snapshot.commit.into());
    }

    Ok(ReferencesData {
        version: latest_version,
        heads: Some(heads),
        bookmarks: Some(bookmarks),
        heads_dates: Some(heads_dates),
        remote_bookmarks: Some(remote_bookmarks),
        snapshots: Some(snapshots),
        timestamp: Some(version_timestamp),
    })
}

pub(crate) async fn update_references_data(
    sql: &SqlCommitCloud,
    params: UpdateReferencesParams,
    ctx: &CommitCloudContext,
) -> anyhow::Result<()> {
    update_heads(sql, ctx.clone(), params.removed_heads, params.new_heads).await?;
    update_bookmarks(
        sql,
        ctx.clone(),
        params.updated_bookmarks,
        params.removed_bookmarks,
    )
    .await?;
    update_remote_bookmarks(
        sql,
        ctx.clone(),
        params.updated_remote_bookmarks,
        params.removed_remote_bookmarks,
    )
    .await?;
    update_snapshots(
        sql,
        ctx.clone(),
        params.new_snapshots,
        params.removed_snapshots,
    )
    .await?;
    Ok(())
}
