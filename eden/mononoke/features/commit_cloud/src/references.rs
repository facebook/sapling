/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Instant;

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
use derivation_queue_thrift::DerivationPriority;
use futures::FutureExt;
use futures::future;
use futures::stream;
use futures::stream::TryStreamExt;
use futures::try_join;
use history::WorkspaceHistory;
use repo_derived_data::ArcRepoDerivedData;
use sql_ext::Transaction;
use stats::prelude::*;
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

define_stats! {
    prefix = "mononoke.commit_cloud.get_references";
    // Total number of heads returned per sync (with a resolved author date).
    // This is the denominator that makes the timers below per-head figures, so
    // it stays as long as any of them do.
    heads_returned: timeseries(Sum, Average, Count),
    // Wall-clock latency of the get_references read path, split into its two
    // phases plus the total. Emitted as quantile_stat (not the deprecated
    // histogram, whose tail percentiles run 2-3x too high) to match the
    // *_duration_ms convention used by the slapi ODS middleware.
    fetch_references_ms: quantile_stat(Average, Sum, Count; P 50, P 75, P 95, P 99; Duration::from_secs(60), Duration::from_secs(600), Duration::from_secs(3600)),
    cast_references_data_ms: quantile_stat(Average, Sum, Count; P 50, P 75, P 95, P 99; Duration::from_secs(60), Duration::from_secs(600), Duration::from_secs(3600)),
    total_ms: quantile_stat(Average, Sum, Count; P 50, P 75, P 95, P 99; Duration::from_secs(60), Duration::from_secs(600), Duration::from_secs(3600)),
    // The two I/O stages inside cast_references_data, as the per-sync sum of time
    // spent inside each stage's futures. The stages are pipelined and run
    // concurrently, so these are sums over overlapping futures: they routinely
    // exceed wall clock and do NOT partition cast_references_data_ms. Compare them
    // to each other, not to it.
    bonsai_mapping_ms: quantile_stat(Average, Sum, Count; P 50, P 75, P 95, P 99; Duration::from_secs(60), Duration::from_secs(600), Duration::from_secs(3600)),
    changeset_info_ms: quantile_stat(Average, Sum, Count; P 50, P 75, P 95, P 99; Duration::from_secs(60), Duration::from_secs(600), Duration::from_secs(3600)),
}

// Emit the number of heads a (non-no-op) get_references resolved author dates
// for. Same always-on, O(1), pure-telemetry contract as the timers below, and
// the per-head denominator for them.
fn log_heads_returned(heads: usize) {
    STATS::heads_returned.add_value(heads as i64);
}

// Emit the per-phase and total wall-clock timing of a (non-no-op) get_references
// read. This is always-on and O(1) -- just a few Instant reads in the caller plus
// these add_value calls -- so it is deliberately not gated by a JustKnob. The
// read path had no sub-phase timing, and profiling shows latency concentrated in
// the tail of rebuild syncs (scaling super-linearly with workspace size); this
// splits fetch_references vs cast_references_data vs total so we can confirm which
// phase dominates before optimizing. Pure telemetry: it must not affect results.
pub(crate) fn log_get_references_timing(fetch_ms: i64, cast_ms: i64, total_ms: i64) {
    STATS::fetch_references_ms.add_value(fetch_ms);
    STATS::cast_references_data_ms.add_value(cast_ms);
    STATS::total_ms.add_value(total_ms);
}

// Emit the split of cast_references_data into its two I/O stages. See the counter
// comments in define_stats!: these are sums over concurrent futures, so their
// ratio identifies the dominant stage but their total is not comparable to
// cast_references_data_ms. Same always-on, O(1), pure-telemetry contract as
// log_get_references_timing.
fn log_cast_phase_timing(bonsai_mapping_ms: i64, changeset_info_ms: i64) {
    STATS::bonsai_mapping_ms.add_value(bonsai_mapping_ms);
    STATS::changeset_info_ms.add_value(changeset_info_ms);
}

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
    // The four reads below are independent SQL queries against different
    // tables (heads/local_bookmarks/remote_bookmarks/snapshots), keyed only
    // by (reponame, workspace) -- none depends on another's result. Issuing
    // them concurrently instead of sequentially cuts this function's wall
    // time from ~4 round trips to ~1 on the `hg cloud sync` hot path (every
    // get_references/update_references call goes through here).
    let (heads, local_bookmarks, remote_bookmarks, snapshots) = try_join!(
        Get::<WorkspaceHead>::get(sql, ctx, cc_ctx.reponame.clone(), cc_ctx.workspace.clone()),
        Get::<WorkspaceLocalBookmark>::get(
            sql,
            ctx,
            cc_ctx.reponame.clone(),
            cc_ctx.workspace.clone()
        ),
        Get::<WorkspaceRemoteBookmark>::get(
            sql,
            ctx,
            cc_ctx.reponame.clone(),
            cc_ctx.workspace.clone()
        ),
        Get::<WorkspaceSnapshot>::get(sql, ctx, cc_ctx.reponame.clone(), cc_ctx.workspace.clone()),
    )?;

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

    // Start the pipeline with batches of 250 heads. Chunk size and the
    // try_flatten_unordered limit below multiply out to the number of heads in
    // flight against bonsai_hg_mapping, so they are tuned as a pair.
    let chunks_iter = raw_references_data.heads.chunks(250).map(|chunk| {
        let chunk_heads: Vec<CloudChangesetId> = chunk.iter().map(|head| head.commit).collect();
        Ok::<_, anyhow::Error>(chunk_heads)
    });

    let repo_derived_data = &repo_derived_data;

    // The two stages below are pipelined, so neither can be bracketed by a single
    // Instant in this function. Each future instead adds its own elapsed time here.
    let bonsai_mapping_nanos = AtomicU64::new(0);
    let changeset_info_nanos = AtomicU64::new(0);
    let bonsai_mapping_nanos = &bonsai_mapping_nanos;
    let changeset_info_nanos = &changeset_info_nanos;

    let heads_dates: HashMap<CloudChangesetId, i64> = stream::iter(chunks_iter)
        // map [CloudChangesetId] to [(CloudChangesetId, BonsaiChangesetId)]
        .and_then(|heads| {
            cloned!(bonsai_hg_mapping, bonsai_git_mapping);
            async move {
                let start = Instant::now();
                let mapped = utils::get_bonsai_from_cloud_ids(
                    core_ctx,
                    cc_ctx,
                    bonsai_hg_mapping,
                    bonsai_git_mapping,
                    heads,
                )
                .await?;
                bonsai_mapping_nanos
                    .fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
                Ok(stream::iter(mapped.into_iter().map(Ok::<_, anyhow::Error>)))
            }
        })
        // do up to 40 hg->bonsai mappings concurrently, flattening out results
        .try_flatten_unordered(40)
        // map (CloudChangesetId, BonsaiChangesetId) to (CloudChangesetId, unix_timestamp)
        .and_then(|(cid, bcs_id)| async move {
            let start = Instant::now();
            let derived = repo_derived_data
                .derive::<ChangesetInfo>(core_ctx, bcs_id, DerivationPriority::LOW)
                .await;
            changeset_info_nanos.fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
            derived
                .map_err(Into::into)
                .map(|cs_info| future::ok((cid, cs_info.author_date().as_chrono().timestamp())))
        })
        // do up to 100 derived data fetches concurrently
        .try_buffer_unordered(100)
        .try_collect()
        .boxed()
        .await?;

    log_cast_phase_timing(
        (bonsai_mapping_nanos.load(Ordering::Relaxed) / 1_000_000) as i64,
        (changeset_info_nanos.load(Ordering::Relaxed) / 1_000_000) as i64,
    );

    log_heads_returned(heads_dates.len());

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
