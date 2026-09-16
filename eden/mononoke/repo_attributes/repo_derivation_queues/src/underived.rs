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
use std::time::Duration;

use anyhow::Result;
use anyhow::anyhow;
use bulk_derivation::BulkDerivation;
use cloned::cloned;
use commit_graph::ChangesetSegment;
use commit_graph::CommitGraph;
use context::CoreContext;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivedDataManager;
use ephemeral_blobstore::BubbleId;
use futures::future;
use futures::future::FutureExt;
use futures::join;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_stats::futures03::TimedFutureExt;
use itertools::Itertools;
use mononoke_types::ChangesetId;
use parking_lot::Mutex;
use tracing::debug;
use tracing::error;

use crate::DagItemDep;
use crate::DerivationDagItem;
use crate::DerivationPriority;
use crate::DerivationQueue;
use crate::EnqueueResponse;
use crate::InternalError;

// Generation number starts with 1, so we need to account for it by offsetting
// We also need to multiply index additionally by (batch size)
// to get the generation number of root for each bat
fn batch_generation_number(cs_generation: u64, batch_size: u64) -> u64 {
    (cs_generation - 1) / batch_size * batch_size + 1
}

#[derive(Clone)]
struct BatchRange {
    root: ChangesetId,
    head: ChangesetId,
}

struct UnderivedBatch {
    head: ChangesetId,
    parents: Vec<ChangesetId>,
}

struct UnderivedSegment {
    segment: ChangesetSegment,
    base_generation: u64,
    head_generation: u64,
    batches: Vec<BatchRange>,
}

struct UnderivedGraphBuildResult {
    response: Option<EnqueueResponse>,
    commits_walked: u64,
    items_enqueued: u64,
}

async fn split_segment_into_batches(
    ctx: &CoreContext,
    commit_graph: Arc<CommitGraph>,
    segment: ChangesetSegment,
    batch_size: u64,
) -> Result<UnderivedSegment> {
    let head_generation = commit_graph
        .changeset_generation(ctx, segment.head)
        .await?
        .value();
    let generation_span = segment
        .length
        .checked_sub(1)
        .ok_or_else(|| anyhow!("commit graph returned an empty segment"))?;
    let base_generation = head_generation
        .checked_sub(generation_span)
        .ok_or_else(|| anyhow!("commit graph returned an invalid segment length"))?;

    let mut boundaries = Vec::new();
    let mut boundary = batch_generation_number(head_generation, batch_size);
    while boundary > base_generation {
        boundaries.push(boundary);
        boundary = boundary
            .checked_sub(batch_size)
            .ok_or_else(|| anyhow!("invalid batch boundary"))?;
    }

    let mut boundary_pairs = stream::iter(boundaries)
        .map(|boundary| {
            cloned!(commit_graph);
            async move {
                let ids = commit_graph
                    .locations_to_changeset_ids(ctx, segment.head, head_generation - boundary, 2)
                    .await?;
                match ids.as_slice() {
                    [upper_root, lower_head] => {
                        Ok((boundary, upper_root.clone(), lower_head.clone()))
                    }
                    _ => Err(anyhow!(
                        "commit graph returned {} changesets for a batch boundary",
                        ids.len()
                    )),
                }
            }
        })
        .buffer_unordered(100)
        .try_collect::<Vec<_>>()
        .await?;
    boundary_pairs.sort_unstable_by_key(|(boundary, _, _)| *boundary);

    let mut batches = Vec::with_capacity(boundary_pairs.len() + 1);
    let mut root = segment.base;
    for (_, upper_root, lower_head) in boundary_pairs {
        batches.push(BatchRange {
            root,
            head: lower_head,
        });
        root = upper_root;
    }
    batches.push(BatchRange {
        root,
        head: segment.head,
    });

    Ok(UnderivedSegment {
        segment,
        base_generation,
        head_generation,
        batches,
    })
}

fn parent_batch_root(
    parent_segment: &UnderivedSegment,
    distance: u64,
    batch_size: u64,
) -> Result<ChangesetId> {
    let parent_generation = parent_segment
        .head_generation
        .checked_sub(distance)
        .ok_or_else(|| anyhow!("segment parent location is invalid"))?;
    let first_batch_generation =
        batch_generation_number(parent_segment.base_generation, batch_size);
    let parent_batch_generation = batch_generation_number(parent_generation, batch_size);
    let parent_batch = parent_batch_generation
        .checked_sub(first_batch_generation)
        .ok_or_else(|| anyhow!("segment parent precedes its segment"))?
        / batch_size;
    let parent_batch = usize::try_from(parent_batch)
        .map_err(|_| anyhow!("segment parent batch index is too large"))?;

    Ok(parent_segment
        .batches
        .get(parent_batch)
        .ok_or_else(|| anyhow!("segment parent batch is missing"))?
        .root)
}

pub async fn build_underived_batched_graph<'a>(
    ctx: &'a CoreContext,
    queue: Arc<dyn DerivationQueue + Send + Sync>,
    ddm: &'a DerivedDataManager,
    derived_data_type: DerivableType,
    head: ChangesetId,
    bubble_id: Option<BubbleId>,
    batch_size: u64,
    priority: Option<DerivationPriority>,
) -> Result<Option<EnqueueResponse>> {
    let (stats, build_result) = build_underived_batched_graph_impl(
        ctx,
        queue,
        ddm,
        derived_data_type,
        head,
        bubble_id,
        batch_size,
        priority,
    )
    .timed()
    .await;
    let build_result = build_result?;

    let mut scuba = ctx.scuba().clone();
    scuba.unsampled();
    scuba.add_future_stats(&stats);
    scuba.add("graph_builder_version", "v2");
    scuba.add("derived_data_type", derived_data_type.name());
    scuba.add("head_cs_id", head.to_string());
    scuba.add("commits_walked", build_result.commits_walked);
    scuba.add("items_enqueued", build_result.items_enqueued);
    ctx.perf_counters().insert_perf_counters(&mut scuba);
    scuba.log_with_msg("Underived graph built", None);

    Ok(build_result.response)
}

async fn build_underived_batched_graph_impl<'a>(
    ctx: &'a CoreContext,
    queue: Arc<dyn DerivationQueue + Send + Sync>,
    ddm: &'a DerivedDataManager,
    derived_data_type: DerivableType,
    head: ChangesetId,
    bubble_id: Option<BubbleId>,
    batch_size: u64,
    priority: Option<DerivationPriority>,
) -> Result<UnderivedGraphBuildResult> {
    let priority = priority.unwrap_or(DerivationPriority::LOW);
    let repo_id = ddm.repo_id();
    let config_name = ddm.config_name();
    let commit_graph = ddm.commit_graph_arc();
    let items_enqueued = Arc::new(AtomicU64::new(0));
    let watch = Arc::new(Mutex::new(Some(EnqueueResponse::new(
        future::ok(false).boxed(),
    ))));

    if batch_size == 0 {
        return Err(anyhow!("batch size must be greater than zero"));
    }

    let parents_derived = stream::iter(commit_graph.changeset_parents(ctx, head).await?)
        .map(|parent| async move {
            anyhow::Ok((
                parent,
                ddm.is_derived(ctx, parent, None, derived_data_type).await?,
            ))
        })
        .buffer_unordered(100)
        .try_collect::<Vec<_>>()
        .await?;
    let underived_parents = parents_derived
        .iter()
        .filter(|(_, derived)| !derived)
        .map(|(parent, _)| *parent)
        .collect::<Vec<_>>();

    // Derived data is expected to be monotonic. Workers recover the rare case
    // where an ancestor's derived-data mapping is missing.
    let (batches, root, commits_walked) = if underived_parents.is_empty() {
        (
            HashMap::from([(
                head,
                UnderivedBatch {
                    head,
                    parents: vec![],
                },
            )]),
            head,
            1,
        )
    } else {
        let known_parents = parents_derived.iter().copied().collect::<HashMap<_, _>>();
        let derived_frontier = commit_graph
            .ancestors_frontier_with(ctx, underived_parents, |cs_id| {
                let known_parents = &known_parents;
                async move {
                    match known_parents.get(&cs_id) {
                        Some(derived) => Ok(*derived),
                        None => Ok(ddm.is_derived(ctx, cs_id, None, derived_data_type).await?),
                    }
                }
            })
            .await?
            .into_iter()
            .chain(
                parents_derived
                    .iter()
                    .filter(|(_, derived)| *derived)
                    .map(|(parent, _)| *parent),
            )
            .collect::<Vec<_>>();
        let segments = commit_graph
            .ancestors_difference_segments(ctx, vec![head], derived_frontier)
            .await?;
        if segments.is_empty() {
            return Ok(UnderivedGraphBuildResult {
                response: Some(EnqueueResponse::new(future::ok(true).boxed())),
                commits_walked: 0,
                items_enqueued: 0,
            });
        }

        let commits_walked = segments.iter().map(|segment| segment.length).sum::<u64>();
        let segments = stream::iter(segments)
            .map(|segment| {
                split_segment_into_batches(ctx, commit_graph.clone(), segment, batch_size)
            })
            .buffer_unordered(100)
            .try_collect::<Vec<_>>()
            .await?;

        let segments_by_head = segments
            .iter()
            .enumerate()
            .map(|(index, segment)| (segment.segment.head, index))
            .collect::<HashMap<_, _>>();
        let mut batches = HashMap::new();
        for segment in &segments {
            for (batch_index, batch) in segment.batches.iter().enumerate() {
                let parents = if batch_index > 0 {
                    vec![segment.batches[batch_index - 1].root]
                } else {
                    segment
                        .segment
                        .parents
                        .iter()
                        .filter_map(|parent| parent.location)
                        .map(|location| {
                            let parent_index = segments_by_head
                                .get(&location.head)
                                .ok_or_else(|| anyhow!("segment parent location is missing"))?;
                            parent_batch_root(
                                &segments[*parent_index],
                                location.distance,
                                batch_size,
                            )
                        })
                        .collect::<Result<Vec<_>>>()?
                        .into_iter()
                        .unique()
                        .collect()
                };
                let underived_batch = UnderivedBatch {
                    head: batch.head,
                    parents,
                };
                batches.insert(batch.root, underived_batch);
            }
        }
        let head_segment = segments_by_head
            .get(&head)
            .map(|index| &segments[*index])
            .ok_or_else(|| anyhow!("underived graph does not contain requested head"))?;
        let root = head_segment
            .batches
            .last()
            .ok_or_else(|| anyhow!("underived head segment has no batches"))?
            .root;
        (batches, root, commits_walked)
    };
    let batches = Arc::new(batches);
    bounded_traversal::bounded_traversal_dag(
        100,
        root,
        {
            cloned!(batches);
            move |root| {
                cloned!(batches);
                async move {
                    let batch = batches
                        .get(&root)
                        .ok_or_else(|| anyhow!("underived graph contains an invalid batch"))?;
                    anyhow::Ok(((root, batch.head), batch.parents.clone()))
                }
                .boxed()
            }
        },
        |(root_cs_id, head_cs_id), deps| {
            cloned!(
                derived_data_type,
                config_name,
                queue,
                commit_graph,
                watch,
                items_enqueued
            );
            async move {
                let item = DerivationDagItem::new(
                    repo_id,
                    config_name.to_string(),
                    derived_data_type,
                    root_cs_id,
                    head_cs_id,
                    bubble_id,
                    deps.flatten().unique().collect(),
                    ctx.metadata().client_info(),
                    priority,
                    None,
                    None,
                )?;

                let max_failed_attempts = justknobs::get_as::<u64>(
                    "scm/mononoke:build_underived_batched_graph_max_failed_attempts",
                    None,
                );

                let mut upstream_dep: Option<DagItemDep> = Some(DagItemDep {
                    dag_item_id: item.id().clone(),
                    head_cs_id: item.head_cs_id(),
                    stage_path: None,
                });
                let mut cur_item = Some(item);
                let mut failed_attempt = 0;
                let mut err_msg = None;
                while let Some(item) = cur_item {
                    if failed_attempt >= max_failed_attempts {
                        return Err(anyhow!(
                            "Couldn't enqueue item {item:?} into zeus after {failed_attempt} attempts. Last err: {err_msg:?}",
                        ));
                    } else if failed_attempt > 0 {
                        let backoff_time =
                            Duration::from_millis(failed_attempt * failed_attempt * 100);
                        tokio::time::sleep(backoff_time).await;
                    }
                    let maybe_inserted = {
                        let enqueue_res = queue.enqueue(ctx, item.clone()).await;
                        match enqueue_res {
                            Ok(resp) => {
                                items_enqueued.fetch_add(1, Ordering::Relaxed);
                                *watch.lock() = Some(resp);
                                None
                            }
                            Err(InternalError::ItemExists(existing)) => {
                                let existing_item_id = item.id().clone();
                                if *existing == item {
                                    *watch.lock() = Some(
                                        queue
                                            .watch_existing(ctx, existing_item_id.clone())
                                            .await?,
                                    );
                                    None
                                } else {
                                    let maybe_dedup = deduplicate(
                                        ctx,
                                        item,
                                        *existing,
                                        bubble_id,
                                        commit_graph.clone(),
                                    )
                                    .await?;
                                    if maybe_dedup.is_none() {
                                        upstream_dep = None;
                                        *watch.lock() = Some(
                                            queue.watch_existing(ctx, existing_item_id).await?,
                                        );
                                    }
                                    maybe_dedup
                                }
                            }
                            Err(e) => {
                                let root_generation = commit_graph
                                    .changeset_generation(ctx, item.root_cs_id())
                                    .await?;
                                let derived_ancestors_or_parents = commit_graph
                                    .ancestors_frontier_with(
                                        ctx,
                                        vec![item.head_cs_id()],
                                        |cs_id| {
                                            cloned!(commit_graph);
                                            async move {
                                                if commit_graph
                                                    .changeset_generation(ctx, cs_id)
                                                    .await?
                                                    < root_generation
                                                {
                                                    Ok(true)
                                                } else {
                                                    Ok(ddm
                                                        .is_derived(
                                                            ctx,
                                                            cs_id,
                                                            None,
                                                            derived_data_type,
                                                        )
                                                        .await?)
                                                }
                                            }
                                        },
                                    )
                                    .await?;

                                let mut underived_batch = commit_graph
                                    .ancestors_difference(
                                        ctx,
                                        vec![item.head_cs_id()],
                                        derived_ancestors_or_parents,
                                    )
                                    .await?;
                                match underived_batch.pop() {
                                    None => {
                                        let err_msg_str = format!(
                                            "Failed to enqueue with error: {e}, but the data was derived"
                                        );
                                        debug!("{}", err_msg_str);
                                        err_msg = Some(err_msg_str);
                                        *watch.lock() = Some(EnqueueResponse::new(
                                            future::ok(true).boxed(),
                                        ));
                                        None
                                    }
                                    Some(root_cs_id) if root_cs_id == item.root_cs_id() => {
                                        failed_attempt += 1;
                                        let err_msg_str =
                                            format!("Failed to enqueue into DAG: {e}");
                                        error!("{}", err_msg_str);
                                        err_msg = Some(err_msg_str);
                                        Some(item)
                                    }
                                    Some(root_cs_id) => Some(DerivationDagItem::new(
                                        item.repo_id(),
                                        item.config_name().to_string(),
                                        item.derived_data_type(),
                                        root_cs_id,
                                        item.head_cs_id(),
                                        item.bubble_id(),
                                        vec![],
                                        item.client_info(),
                                        priority,
                                        None,
                                        None,
                                    )?),
                                }
                            }
                        }
                    };
                    cur_item = maybe_inserted.inspect(|item| {
                        upstream_dep = Some(DagItemDep {
                            dag_item_id: item.id().clone(),
                            head_cs_id: item.head_cs_id(),
                            stage_path: None,
                        });
                    });
                }

                anyhow::Ok(upstream_dep)
            }
            .boxed()
        },
    )
    .await?;

    let mut res = watch.lock();
    Ok(UnderivedGraphBuildResult {
        response: res.take(),
        commits_walked,
        items_enqueued: items_enqueued.load(Ordering::Relaxed),
    })
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
            vec![DagItemDep {
                dag_item_id: existing.id().clone(),
                head_cs_id: existing.head_cs_id(),
                stage_path: existing.stage_payload().and_then(|p| p.path().cloned()),
            }],
            ctx.metadata().client_info(),
            rejected.info().priority(),
            None,
            None, // stage_payload
        )?;
        return Ok(Some(item));
    }
    Ok(None)
}
