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
use clientinfo::ClientRequestInfo;
use commit_cloud_types::references::WorkspaceRemoteBookmark;
use commit_cloud_types::ReferencesData;
use commit_cloud_types::UpdateReferencesParams;
use commit_cloud_types::WorkspaceCheckoutLocation;
use commit_cloud_types::WorkspaceHead;
use commit_cloud_types::WorkspaceLocalBookmark;
use commit_cloud_types::WorkspaceSnapshot;
use context::CoreContext;
use history::WorkspaceHistory;
use mercurial_types::HgChangesetId;
use repo_derived_data::ArcRepoDerivedData;
use sql::Transaction;
use versions::WorkspaceVersion;

use crate::references::heads::update_heads;
use crate::references::local_bookmarks::update_bookmarks;
use crate::references::remote_bookmarks::update_remote_bookmarks;
use crate::references::snapshots::update_snapshots;
use crate::sql::common::UpdateWorkspaceNameArgs;
use crate::sql::ops::Get;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;
use crate::sql::versions_ops::UpdateVersionArgs;
use crate::CommitCloudContext;

pub mod heads;
pub mod history;
pub mod local_bookmarks;
pub mod remote_bookmarks;
pub mod snapshots;
pub mod versions;

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
    ctx: &CommitCloudContext,
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
    let mut heads: Vec<HgChangesetId> = Vec::new();
    let mut bookmarks: HashMap<String, HgChangesetId> = HashMap::new();
    let mut heads_dates: HashMap<HgChangesetId, i64> = HashMap::new();
    let remote_bookmarks: Vec<WorkspaceRemoteBookmark> = raw_references_data.remote_bookmarks;
    let mut snapshots: Vec<HgChangesetId> = Vec::new();

    for head in raw_references_data.heads {
        heads.push(head.commit);
        let bonsai = bonsai_hg_mapping
            .get_bonsai_from_hg(core_ctx, head.commit)
            .await?;
        match bonsai {
            Some(bonsai) => {
                let cs_info = repo_derived_data
                    .derive::<ChangesetInfo>(core_ctx, bonsai.clone())
                    .await?;
                let cs_date = cs_info.author_date();
                heads_dates.insert(head.commit, cs_date.as_chrono().timestamp());
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
        bookmarks.insert(bookmark.name().clone(), *bookmark.commit());
    }

    for snapshot in raw_references_data.snapshots {
        snapshots.push(snapshot.commit);
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
    txn: Transaction,
    cri: Option<&ClientRequestInfo>,
    params: UpdateReferencesParams,
    ctx: &CommitCloudContext,
) -> anyhow::Result<Transaction> {
    let mut txn = txn;
    txn = update_heads(sql, txn, cri, ctx, params.removed_heads, params.new_heads).await?;
    txn = update_bookmarks(
        sql,
        txn,
        cri,
        ctx,
        params.updated_bookmarks,
        params.removed_bookmarks,
    )
    .await?;
    txn = update_remote_bookmarks(
        sql,
        txn,
        cri,
        ctx,
        params.updated_remote_bookmarks,
        params.removed_remote_bookmarks,
    )
    .await?;
    txn = update_snapshots(
        sql,
        txn,
        cri,
        ctx,
        params.new_snapshots,
        params.removed_snapshots,
    )
    .await?;
    Ok(txn)
}

pub async fn rename_all(
    sql: &SqlCommitCloud,
    cri: Option<&ClientRequestInfo>,
    cc_ctx: &CommitCloudContext,
    new_workspace: &str,
) -> anyhow::Result<(Transaction, u64)> {
    let args = UpdateWorkspaceNameArgs {
        new_workspace: new_workspace.to_string(),
    };
    let mut txn = sql.connections.write_connection.start_transaction().await?;

    (txn, _) = Update::<WorkspaceHead>::update(sql, txn, cri, cc_ctx.clone(), args.clone()).await?;
    (txn, _) =
        Update::<WorkspaceLocalBookmark>::update(sql, txn, cri, cc_ctx.clone(), args.clone())
            .await?;
    (txn, _) =
        Update::<WorkspaceRemoteBookmark>::update(sql, txn, cri, cc_ctx.clone(), args.clone())
            .await?;
    (txn, _) =
        Update::<WorkspaceSnapshot>::update(sql, txn, cri, cc_ctx.clone(), args.clone()).await?;
    (txn, _) =
        Update::<WorkspaceCheckoutLocation>::update(sql, txn, cri, cc_ctx.clone(), args.clone())
            .await?;
    (txn, _) =
        Update::<WorkspaceHistory>::update(sql, txn, cri, cc_ctx.clone(), args.clone()).await?;
    let (txn, affected_rows) = Update::<WorkspaceVersion>::update(
        sql,
        txn,
        cri,
        cc_ctx.clone(),
        UpdateVersionArgs::WorkspaceName(new_workspace.to_string()),
    )
    .await?;
    Ok((txn, affected_rows))
}
