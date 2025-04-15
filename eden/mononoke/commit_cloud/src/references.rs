/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_hg_mapping::BonsaiHgMappingEntry;
use changeset_info::ChangesetInfo;
use clientinfo::ClientRequestInfo;
use commit_cloud_types::ReferencesData;
use commit_cloud_types::UpdateReferencesParams;
use commit_cloud_types::WorkspaceCheckoutLocation;
use commit_cloud_types::WorkspaceHead;
use commit_cloud_types::WorkspaceLocalBookmark;
use commit_cloud_types::WorkspaceSnapshot;
use commit_cloud_types::references::WorkspaceRemoteBookmark;
use context::CoreContext;
use futures::FutureExt;
use futures::future;
use futures::stream;
use futures::stream::TryStreamExt;
use history::WorkspaceHistory;
use mercurial_types::HgChangesetId;
use repo_derived_data::ArcRepoDerivedData;
use sql::Transaction;
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
    let mut bookmarks: HashMap<String, HgChangesetId> = HashMap::new();
    let remote_bookmarks: Vec<WorkspaceRemoteBookmark> = raw_references_data.remote_bookmarks;
    let mut snapshots: Vec<HgChangesetId> = Vec::new();

    // Start the pipeline with batches of 1000 heads.
    let chunks_iter = raw_references_data.heads.chunks(1000).map(|chunk| {
        let chunk_heads: Vec<_> = chunk.iter().map(|head| head.commit).collect();
        Ok::<_, anyhow::Error>(chunk_heads)
    });

    let repo_derived_data = &repo_derived_data;
    let bonsai_hg_mapping = &bonsai_hg_mapping;

    let heads_dates: HashMap<HgChangesetId, i64> = stream::iter(chunks_iter)
        // map [HgChangesetId] to [(HgChangesetId, BonsaiChangesetId)]
        .and_then(|heads| async move {
            Ok(stream::iter(
                bonsai_hg_mapping
                    .get(core_ctx, heads.into())
                    .await?
                    .into_iter()
                    .map(Ok::<_, anyhow::Error>),
            ))
        })
        // do up to 10 hg->bonsai mappings concurrently, flattening out results
        .try_flatten_unordered(10)
        // map (HgChangesetId, BonsaiChangesetId) to (HgChangesetId, unix_timestamp)
        .and_then(|BonsaiHgMappingEntry { hg_cs_id, bcs_id }| async move {
            repo_derived_data
                .derive::<ChangesetInfo>(core_ctx, bcs_id)
                .await
                .map_err(Into::into)
                .map(|cs_info| {
                    future::ok((hg_cs_id, cs_info.author_date().as_chrono().timestamp()))
                })
        })
        // do up to 100 derived data fetches concurrently
        .try_buffer_unordered(100)
        .try_collect()
        .boxed()
        .await?;

    for bookmark in raw_references_data.local_bookmarks {
        bookmarks.insert(bookmark.name().clone(), *bookmark.commit());
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
