/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use changeset_info::ChangesetInfo;
use cloned::cloned;
use commit_cloud_types::ReferencesData;
use commit_cloud_types::UpdateReferencesParams;
use commit_cloud_types::WorkspaceCheckoutLocation;
use commit_cloud_types::WorkspaceHead;
use commit_cloud_types::WorkspaceLocalBookmark;
use commit_cloud_types::WorkspaceSnapshot;
use commit_cloud_types::changeset::CloudChangesetId;
use commit_cloud_types::references::WorkspaceRemoteBookmark;
use context::CoreContext;
use futures::FutureExt;
use futures::future;
use futures::stream;
use futures::stream::TryStreamExt;
use history::WorkspaceHistory;
use repo_derived_data::ArcRepoDerivedData;
use sql_ext::Transaction;
use versions::WorkspaceVersion;

use crate::CommitCloudContext;
use crate::references::heads::update_heads;
use crate::references::local_bookmarks::update_bookmarks;
use crate::references::remote_bookmarks::update_remote_bookmarks;
use crate::references::snapshots::update_snapshots;
use crate::sql::common::UpdateWorkspaceNameArgs;
use crate::sql::ops::Get;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;
use crate::sql::versions_ops::UpdateVersionArgs;
use crate::utils;

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
    ctx: &CoreContext,
    cc_ctx: &CommitCloudContext,
    sql: &SqlCommitCloud,
) -> Result<RawReferencesData, anyhow::Error> {
    let heads: Vec<WorkspaceHead> = sql
        .get(ctx, cc_ctx.reponame.clone(), cc_ctx.workspace.clone())
        .await?;

    let local_bookmarks: Vec<WorkspaceLocalBookmark> = sql
        .get(ctx, cc_ctx.reponame.clone(), cc_ctx.workspace.clone())
        .await?;

    let remote_bookmarks: Vec<WorkspaceRemoteBookmark> = sql
        .get(ctx, cc_ctx.reponame.clone(), cc_ctx.workspace.clone())
        .await?;

    let snapshots: Vec<WorkspaceSnapshot> = sql
        .get(ctx, cc_ctx.reponame.clone(), cc_ctx.workspace.clone())
        .await?;

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
    bonsai_git_mapping: Arc<dyn BonsaiGitMapping>,
    repo_derived_data: ArcRepoDerivedData,
    core_ctx: &CoreContext,
    cc_ctx: &CommitCloudContext,
) -> Result<ReferencesData, anyhow::Error> {
    let mut bookmarks: HashMap<String, CloudChangesetId> = HashMap::new();
    let remote_bookmarks: Vec<WorkspaceRemoteBookmark> = raw_references_data.remote_bookmarks;
    let mut snapshots: Vec<CloudChangesetId> = Vec::new();

    // Start the pipeline with batches of 1000 heads.
    let chunks_iter = raw_references_data.heads.chunks(1000).map(|chunk| {
        let chunk_heads: Vec<CloudChangesetId> = chunk.iter().map(|head| head.commit).collect();
        Ok::<_, anyhow::Error>(chunk_heads)
    });

    let repo_derived_data = &repo_derived_data;

    let heads_dates: HashMap<CloudChangesetId, i64> = stream::iter(chunks_iter)
        // map [CloudChangesetId] to [(CloudChangesetId, BonsaiChangesetId)]
        .and_then(|heads| {
            cloned!(bonsai_hg_mapping, bonsai_git_mapping);
            async move {
                Ok(stream::iter(
                    utils::get_bonsai_from_cloud_ids(
                        core_ctx,
                        cc_ctx,
                        bonsai_hg_mapping,
                        bonsai_git_mapping,
                        heads,
                    )
                    .await?
                    .into_iter()
                    .map(Ok::<_, anyhow::Error>),
                ))
            }
        })
        // do up to 10 hg->bonsai mappings concurrently, flattening out results
        .try_flatten_unordered(10)
        // map (CloudChangesetId, BonsaiChangesetId) to (CloudChangesetId, unix_timestamp)
        .and_then(|(cid, bcs_id)| async move {
            repo_derived_data
                .derive::<ChangesetInfo>(core_ctx, bcs_id)
                .await
                .map_err(Into::into)
                .map(|cs_info| future::ok((cid, cs_info.author_date().as_chrono().timestamp())))
        })
        // do up to 100 derived data fetches concurrently
        .try_buffer_unordered(100)
        .try_collect()
        .boxed()
        .await?;

    for bookmark in raw_references_data.local_bookmarks {
        bookmarks.insert(bookmark.name().clone(), bookmark.commit().clone());
    }

    for snapshot in raw_references_data.snapshots {
        snapshots.push(snapshot.commit);
    }

    Ok(ReferencesData {
        version: latest_version,
        heads: Some(
            raw_references_data
                .heads
                .iter()
                .map(|head| head.commit)
                .collect(),
        ),
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
    ctx: &CoreContext,
    params: UpdateReferencesParams,
    cc_ctx: &CommitCloudContext,
) -> anyhow::Result<Transaction> {
    let mut txn = txn;
    txn = update_heads(
        sql,
        txn,
        ctx,
        cc_ctx,
        params.removed_heads,
        params.new_heads,
    )
    .await?;
    txn = update_bookmarks(
        sql,
        txn,
        ctx,
        cc_ctx,
        params.updated_bookmarks,
        params.removed_bookmarks,
    )
    .await?;
    txn = update_remote_bookmarks(
        sql,
        txn,
        ctx,
        cc_ctx,
        params.updated_remote_bookmarks,
        params.removed_remote_bookmarks,
    )
    .await?;
    txn = update_snapshots(
        sql,
        txn,
        ctx,
        cc_ctx,
        params.new_snapshots,
        params.removed_snapshots,
    )
    .await?;
    Ok(txn)
}

pub async fn rename_all(
    sql: &SqlCommitCloud,
    ctx: &CoreContext,
    cc_ctx: &CommitCloudContext,
    new_workspace: &str,
) -> anyhow::Result<(Transaction, u64)> {
    let args = UpdateWorkspaceNameArgs {
        new_workspace: new_workspace.to_string(),
    };
    let mut txn = sql
        .connections
        .write_connection
        .start_transaction(ctx.sql_query_telemetry())
        .await?;

    (txn, _) = Update::<WorkspaceHead>::update(sql, txn, ctx, cc_ctx.clone(), args.clone()).await?;
    (txn, _) =
        Update::<WorkspaceLocalBookmark>::update(sql, txn, ctx, cc_ctx.clone(), args.clone())
            .await?;
    (txn, _) =
        Update::<WorkspaceRemoteBookmark>::update(sql, txn, ctx, cc_ctx.clone(), args.clone())
            .await?;
    (txn, _) =
        Update::<WorkspaceSnapshot>::update(sql, txn, ctx, cc_ctx.clone(), args.clone()).await?;
    (txn, _) =
        Update::<WorkspaceCheckoutLocation>::update(sql, txn, ctx, cc_ctx.clone(), args.clone())
            .await?;
    (txn, _) =
        Update::<WorkspaceHistory>::update(sql, txn, ctx, cc_ctx.clone(), args.clone()).await?;
    let (txn, affected_rows) = Update::<WorkspaceVersion>::update(
        sql,
        txn,
        ctx,
        cc_ctx.clone(),
        UpdateVersionArgs::WorkspaceName(new_workspace.to_string()),
    )
    .await?;
    Ok((txn, affected_rows))
}
