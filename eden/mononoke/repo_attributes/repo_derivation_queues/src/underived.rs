/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Result;
use bulk_derivation::BulkDerivation;
use cloned::cloned;
use commit_graph::CommitGraph;
use context::CoreContext;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivedDataManager;
use ephemeral_blobstore::BubbleId;
use futures::future;
use futures::future::FutureExt;
use futures::join;
use futures::stream::StreamExt;
use mononoke_types::ChangesetId;
use parking_lot::Mutex;
use slog::debug;
use slog::error;

use crate::DerivationDagItem;
use crate::DerivationQueue;
use crate::EnqueueResponse;
use crate::InternalError;

// Generation number starts with 1, so we need to account for it by offsetting
// We also need to multiply index additionally by (batch size)
// to get the generation number of root for each bat
fn batch_generation_number(cs_generation: u64, batch_size: u64) -> u64 {
    (cs_generation - 1) / batch_size * batch_size + 1
}

pub async fn build_underived_batched_graph<'a>(
    ctx: &'a CoreContext,
    queue: Arc<dyn DerivationQueue + Send + Sync>,
    ddm: &'a DerivedDataManager,
    derived_data_type: DerivableType,
    head: ChangesetId,
    bubble_id: Option<BubbleId>,
    batch_size: u64,
) -> Result<Option<EnqueueResponse>> {
    let repo_id = ddm.repo_id();
    let config_name = ddm.config_name();
    let commit_graph = ddm.commit_graph_arc();
    let watch = Arc::new(Mutex::new(Some(EnqueueResponse::new(Box::new(
        future::ok(false),
    )))));
    let _ = bounded_traversal::bounded_traversal_dag(
        100,
        head,
        |cs| {
            cloned!(commit_graph, derived_data_type);
            async move {
                // Walk down by parent until batch full or found merge or derived
                let mut root = cs;
                let head = cs;
                let generation = commit_graph.changeset_generation(ctx, cs).await?;

                let cur_batch_index = batch_generation_number(generation.value(), batch_size);
                let mut next = Vec::new();
                loop {
                    let parents = commit_graph.changeset_parents(ctx, root).await?;
                    // Gather underived parents for the current changeset.
                    let mut underived_parents = Vec::new();
                    for parent_cs in parents.clone() {
                        if !ddm.is_derived(ctx, parent_cs, None, derived_data_type).await? {
                            underived_parents.push(parent_cs);
                        }
                    }
                    // All parents are derived, we found last underived commit
                    if underived_parents.is_empty() {
                        break;
                    }
                    // Merge commit, always break batch
                    if parents.len() > 1 {
                        next = underived_parents;
                        break;
                    }
                    // Non-merge commit, break batch at generation boundary
                    let parent_cs = parents.first().expect("Parent should exist").clone();
                    let parent_generation = commit_graph
                        .changeset_generation(ctx, parent_cs)
                        .await?;
                    let parent_batch_index =
                        batch_generation_number(parent_generation.value(), batch_size);
                    if parent_batch_index != cur_batch_index {
                        // Parent should be in different batch
                        next = vec![parent_cs];
                        break;
                    }
                    // Add parent to the current batch
                    root = parent_cs;
                }
                anyhow::Ok(((root, head), next))
            }
            .boxed()
        },
        |(root_cs_id, head_cs_id), deps| {
            cloned!(
                derived_data_type,
                config_name,
                queue,
                commit_graph,
                watch
            );
            async move {
                let item = DerivationDagItem::new(
                    repo_id,
                    config_name.to_string(),
                    derived_data_type,
                    root_cs_id,
                    head_cs_id,
                    bubble_id,
                    deps.collect(),
                    ctx.metadata().client_info(),
                )?;

                let max_failed_attemps = justknobs::get_as::<u64>("scm/mononoke:build_underived_batched_graph_max_failed_attempts", None)?;

                // Upstream batch will depend on this cs
                let mut upstream_dep = item.id().clone();
                let mut cur_item = Some(item);
                let mut failed_attempt = 0;
                let mut err_msg = None;
                while let Some(item) = cur_item {
                    if failed_attempt >= max_failed_attemps {
                        return Err(anyhow!(
                            "Couldn't enqueue item {:?} into zeus after {} attempts. Last err: {:?}",
                            item,
                            failed_attempt,
                            err_msg,
                        ));
                    } else if failed_attempt > 0 {
                        let backoff_time = Duration::from_millis(failed_attempt * failed_attempt * 100);
                        tokio::time::sleep(backoff_time).await;
                    }
                    let maybe_inserted = {
                        let enqueue_res = queue.enqueue(ctx, item.clone()).await;
                        match enqueue_res {
                            Ok(resp) => {
                                *watch.lock() = Some(resp);
                                None
                            }
                            Err(InternalError::ItemExists(existing)) => {
                                // Item already in DAG, another reqeust for derivation trigger that
                                // we need to return watch for this existing item.
                                let existing_item_id = item.id().clone();
                                if *existing == item {
                                    *watch.lock() =
                                        Some(queue.watch_existing(ctx, existing_item_id.clone()).await?);
                                    None
                                } else {
                                    // Items are different, we need to deduplicate or discard
                                    let maybe_dedup = deduplicate(ctx, item, *existing, bubble_id, commit_graph.clone())
                                        .await?;
                                    // We couldn't deduplicate because rejected commits are in the existing item
                                    // set watch for existing item
                                    if maybe_dedup.is_none() {
                                        *watch.lock() =
                                            Some(queue.watch_existing(ctx, existing_item_id).await?);
                                    }
                                    maybe_dedup
                                }
                            }
                            Err(e) => {
                                let root_generation = commit_graph.changeset_generation(ctx, item.root_cs_id()).await?;
                                // Find the highest derived changeset in the batch or the parents of the batch
                                // if none of the changesets are derived.
                                let derived_ancestors_or_parents = commit_graph.ancestors_frontier_with(ctx, vec![item.head_cs_id()],
                                    |cs_id| {
                                        cloned!(commit_graph);
                                        async move {
                                            if commit_graph.changeset_generation(ctx, cs_id).await? < root_generation {
                                                Ok(true)
                                            } else {
                                                Ok(ddm.is_derived(ctx, cs_id, None, derived_data_type).await?)
                                            }
                                        }
                                    }
                                )
                                .await?;

                                let mut underived_batch = commit_graph.ancestors_difference(ctx, vec![item.head_cs_id()], derived_ancestors_or_parents).await?;
                                match underived_batch.pop() {
                                    // All changesets in the batch were derived
                                    None => {
                                        let err_msg_str = format!("Failed to enqueue with error: {}, but the data was derived", e);
                                        debug!(ctx.logger(), "{}", err_msg_str);
                                        err_msg = Some(err_msg_str);
                                        // derived, update ready watch and return no dependency
                                        *watch.lock() =
                                            Some(EnqueueResponse::new(Box::new(future::ok(true))));
                                        None
                                    }
                                    // None of the changesets in the batch were derived, but enqueuing failed
                                    Some(root_cs_id) if root_cs_id == item.root_cs_id() => {
                                        // return same item for enqueue and increment failures count
                                        failed_attempt += 1;
                                        let err_msg_str = format!("Failed to enqueue into DAG: {}", e);
                                        error!(ctx.logger(), "{}", err_msg_str);
                                        err_msg = Some(err_msg_str);
                                        Some(item)
                                    }
                                    // Some of the changesets in the batch were derived
                                    Some(root_cs_id) => {
                                        // Create a new item with only the underived changesets
                                        Some(
                                            DerivationDagItem::new(
                                                item.repo_id(),
                                                item.config_name().to_string(),
                                                item.derived_data_type(),
                                                root_cs_id,
                                                item.head_cs_id(),
                                                item.bubble_id(),
                                                vec![],
                                                item.client_info(),
                                            )?
                                        )
                                    }
                                }
                            }
                        }
                    };
                    cur_item = maybe_inserted.inspect(|item| {
                        upstream_dep = item.id().clone();
                    });
                }

                anyhow::Ok(upstream_dep)
            }
            .boxed()
        },
    )
    .await?;
    let mut res = watch.lock();
    Ok(res.take())
}

async fn deduplicate(
    ctx: &CoreContext,
    rejected: DerivationDagItem,
    existing: DerivationDagItem,
    bubble_id: Option<BubbleId>,
    commit_graph: Arc<CommitGraph>,
) -> Result<Option<DerivationDagItem>> {
    assert_eq!(
        rejected.root_cs_id(),
        existing.root_cs_id(),
        "Root cs_id of the duplicated items should be equal"
    );
    let (rejected_ids, existing_ids) = join!(
        commit_graph
            .range_stream(ctx, rejected.root_cs_id(), rejected.head_cs_id())
            .await?
            .collect::<Vec<_>>(),
        commit_graph
            .range_stream(ctx, existing.root_cs_id(), existing.head_cs_id())
            .await?
            .collect::<Vec<_>>(),
    );
    // range_stream returns vector in order from parents to children (Root -> Head)
    // first elements of returned ranges should be equal.
    // We are skipping the common part. Remaining part of rejected range
    // will form new Derivation Item which will depend on existing. If rejected range
    // is smaller than existing iterator will yield None.
    assert!(!rejected_ids.is_empty());
    assert!(!existing_ids.is_empty());
    assert_eq!(rejected_ids.first(), existing_ids.first());
    let mut existing_iter = existing_ids.into_iter();
    let dedup_ids: Vec<_> = rejected_ids
        .into_iter()
        .skip_while(|x| {
            if let Some(next) = existing_iter.next() {
                &next == x
            } else {
                false
            }
        })
        .collect();
    if let (Some(dedup_head), Some(dedup_root)) =
        (dedup_ids.last().cloned(), dedup_ids.first().cloned())
    {
        let item = DerivationDagItem::new(
            rejected.repo_id(),
            rejected.config_name().to_string(),
            rejected.derived_data_type().clone(),
            dedup_root,
            dedup_head,
            bubble_id,
            vec![existing.id().clone()],
            ctx.metadata().client_info(),
        )?;
        return Ok(Some(item));
    }
    Ok(None)
}
