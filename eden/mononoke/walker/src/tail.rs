/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::checkpoint::{Checkpoint, CheckpointsByName};
use crate::graph::{ChangesetKey, Node, NodeType};
use crate::log;
use crate::setup::JobWalkParams;
use crate::state::InternedType;
use crate::walk::{
    walk_exact, OutgoingEdge, RepoWalkParams, RepoWalkTypeParams, StepRoute, TailingWalkVisitor,
    WalkVisitor,
};

use anyhow::{anyhow, bail, Error};
use bulkops::{Direction, PublicChangesetBulkFetch, MAX_FETCH_STEP, MIN_FETCH_STEP};
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    future::{self, Future},
    stream::{self, BoxStream, StreamExt, TryStreamExt},
};
use mononoke_types::{ChangesetId, Timestamp};
use slog::info;
use std::{
    cmp::{max, min},
    collections::HashSet,
    sync::Arc,
};
use strum::IntoEnumIterator;
use tokio::time::{Duration, Instant};

// We can chose to go direct from the ChangesetId to types keyed by it without loading the Changeset
fn roots_for_chunk(
    ids: HashSet<ChangesetId>,
    root_types: &HashSet<NodeType>,
) -> Result<Vec<OutgoingEdge>, Error> {
    let mut edges = vec![];
    for id in ids {
        for r in root_types {
            let n = match r {
                NodeType::BonsaiHgMapping => Node::BonsaiHgMapping(ChangesetKey {
                    inner: id,
                    filenode_known_derived: false,
                }),
                NodeType::PhaseMapping => Node::PhaseMapping(id),
                NodeType::Changeset => Node::Changeset(ChangesetKey {
                    inner: id,
                    filenode_known_derived: false,
                }),
                NodeType::ChangesetInfo => Node::ChangesetInfo(id),
                NodeType::ChangesetInfoMapping => Node::ChangesetInfoMapping(id),
                NodeType::DeletedManifestMapping => Node::DeletedManifestMapping(id),
                NodeType::FsnodeMapping => Node::FsnodeMapping(id),
                NodeType::SkeletonManifestMapping => Node::SkeletonManifestMapping(id),
                NodeType::UnodeMapping => Node::UnodeMapping(id),
                _ => bail!("Unsupported root type for chunking {:?}", r),
            };
            if let Some(edge_type) = n.get_type().root_edge_type() {
                let edge = OutgoingEdge::new(edge_type, n);
                edges.push(edge);
            } else {
                bail!("Unsupported node type for root edges {:?}", n.get_type());
            }
        }
    }
    Ok(edges)
}

#[derive(Clone, Debug)]
pub struct TailParams {
    pub tail_secs: Option<u64>,
    pub public_changeset_chunk_size: Option<usize>,
    pub public_changeset_chunk_by: HashSet<NodeType>,
    pub clear_interned_types: HashSet<InternedType>,
    pub clear_node_types: HashSet<NodeType>,
    pub clear_sample_rate: Option<u64>,
    pub checkpoints: Option<CheckpointsByName>,
    pub state_max_age: Duration,
    pub checkpoint_sample_rate: u64,
}

pub async fn walk_exact_tail<RunFac, SinkFac, SinkOut, V, VOut, Route>(
    fb: FacebookInit,
    job_params: JobWalkParams,
    mut repo_params: RepoWalkParams,
    type_params: RepoWalkTypeParams,
    tail_params: TailParams,
    mut visitor: V,
    make_run: RunFac,
) -> Result<(), Error>
where
    RunFac: 'static + Clone + Send + Sync + FnOnce(&CoreContext, &RepoWalkParams) -> SinkFac,
    SinkFac: 'static + FnOnce(BoxStream<'static, Result<VOut, Error>>) -> SinkOut + Clone + Send,
    SinkOut: Future<Output = Result<(), Error>> + 'static + Send,
    V: 'static + TailingWalkVisitor + WalkVisitor<VOut, Route> + Send + Sync,
    VOut: 'static + Send,
    Route: 'static + Send + Clone + StepRoute,
{
    let repo_id = repo_params.repo.get_repoid();

    let mut state_start = Timestamp::now();

    loop {
        cloned!(job_params, tail_params, type_params, make_run);
        let tail_secs = tail_params.tail_secs;
        // Each loop get new ctx and thus session id so we can distinguish runs
        let ctx = CoreContext::new_with_logger(fb, repo_params.logger.clone());
        let session_text = ctx.session().metadata().session_id().to_string();
        repo_params.scuba_builder.add("session", session_text);

        let mut checkpoint = if let Some(checkpoints) = tail_params.checkpoints.as_ref() {
            checkpoints.load(repo_id).await?
        } else {
            None
        };

        if let Some(cp) = checkpoint.as_ref() {
            info!(repo_params.logger, #log::CHUNKING, "Found checkpoint with bounds: ({}, {})", cp.lower_bound, cp.upper_bound);
        }

        let chunk_params = tail_params
            .public_changeset_chunk_size
            .map(|chunk_size| {
                // Don't SQL fetch in really small or large chunks
                let chunk_size = chunk_size as u64;
                let fetch_step = if chunk_size < MIN_FETCH_STEP {
                    MIN_FETCH_STEP
                } else if chunk_size > MAX_FETCH_STEP {
                    MAX_FETCH_STEP
                } else {
                    chunk_size
                };
                let heads_fetcher = PublicChangesetBulkFetch::new(
                    repo_params.repo.get_repoid(),
                    repo_params.repo.get_changesets_object(),
                    repo_params.repo.get_phases(),
                )
                .with_read_from_master(false)
                .with_step(fetch_step);
                heads_fetcher.map(|v| (chunk_size as usize, v))
            })
            .transpose()?;

        let is_chunking = chunk_params.is_some();

        // Get the chunk stream and whether the bounds it covers are contiguous
        let (contiguous_bounds, mut best_bounds, chunk_stream) = if let Some((
            chunk_size,
            heads_fetcher,
        )) = &chunk_params
        {
            let (lower, upper) = heads_fetcher.get_repo_bounds().await?;
            info!(repo_params.logger, #log::CHUNKING, "Repo bounds: ({}, {})", lower, upper);

            let (contiguous_bounds, best_low, catchup_bounds, main_bounds) = if let Some(
                ref mut checkpoint,
            ) = checkpoint
            {
                let age_secs = checkpoint.create_timestamp.since_seconds();
                if age_secs >= 0 && Duration::from_secs(age_secs as u64) > tail_params.state_max_age
                {
                    info!(repo_params.logger, #log::CHUNKING, "Checkpoint is too old at {}s, running from repo bounds", age_secs);
                    // Reset checkpoints create_timestamp as we're starting again
                    checkpoint.create_timestamp = Timestamp::now();
                    (true, None, None, Some((lower, upper)))
                } else {
                    let (catchup_bounds, main_bounds) = checkpoint.stream_bounds(lower, upper)?;

                    let contiguous_bounds = match (catchup_bounds, main_bounds) {
                        (Some((catchup_lower, _)), Some((_, main_upper))) => {
                            catchup_lower == main_upper
                        }
                        (Some(_), None) => false,
                        _ => true,
                    };
                    info!(repo_params.logger, #log::CHUNKING, "Continuing from checkpoint with catchup {:?} and main {:?} bounds", catchup_bounds, main_bounds);
                    (
                        contiguous_bounds,
                        Some(checkpoint.lower_bound),
                        catchup_bounds,
                        main_bounds,
                    )
                }
            } else {
                (true, None, None, Some((lower, upper)))
            };

            let load_ids = |(lower, upper)| {
                heads_fetcher
                    .fetch_ids(&ctx, Direction::NewestFirst, Some((lower, upper)))
                    .chunks(*chunk_size)
                    .map(move |v| v.into_iter().collect::<Result<HashSet<_>, Error>>())
            };

            let main_s = if let Some(main_bounds) = main_bounds {
                load_ids(main_bounds).left_stream()
            } else {
                stream::once(future::ok(HashSet::new())).right_stream()
            };

            let s = if let Some(catchup_bounds) = catchup_bounds {
                load_ids(catchup_bounds).chain(main_s).left_stream()
            } else {
                main_s.right_stream()
            };
            (contiguous_bounds, Some((best_low, upper)), s.left_stream())
        } else {
            let s = stream::once(future::ok(HashSet::new())).right_stream();
            (true, None, s)
        };

        let mut chunk_num: u64 = 0;

        futures::pin_mut!(chunk_stream);
        while let Some(chunk_members) = chunk_stream.try_next().await? {
            if is_chunking && chunk_members.is_empty() {
                continue;
            }
            chunk_num += 1;

            // convert from stream of (id, bounds) to ids plus overall bounds
            let mut chunk_low: u64 = u64::MAX;
            let mut chunk_upper: u64 = 0;
            let chunk_members = chunk_members
                .into_iter()
                .map(|(cs_id, (fetch_low, fetch_upper))| {
                    chunk_low = min(chunk_low, fetch_low);
                    chunk_upper = max(chunk_upper, fetch_upper);
                    cs_id
                })
                .collect();

            cloned!(repo_params.logger);
            if is_chunking {
                info!(logger, #log::CHUNKING, "Starting chunk {} with bounds ({}, {})", chunk_num, chunk_low, chunk_upper);
            }

            cloned!(mut repo_params);
            let extra_roots = visitor.start_chunk(&chunk_members)?.into_iter();
            let chunk_roots =
                roots_for_chunk(chunk_members, &tail_params.public_changeset_chunk_by)?;
            repo_params.walk_roots.extend(chunk_roots);
            repo_params.walk_roots.extend(extra_roots);

            cloned!(ctx, job_params, make_run, type_params);
            let make_sink = make_run(&ctx, &repo_params);

            // Walk needs clonable visitor, so wrap in Arc for its duration
            let arc_v = Arc::new(visitor);
            let walk_output = walk_exact(ctx, arc_v.clone(), job_params, repo_params, type_params);
            make_sink(walk_output).await?;
            visitor = Arc::try_unwrap(arc_v).map_err(|_| anyhow!("could not unwrap visitor"))?;

            if is_chunking {
                info!(logger, #log::LOADED, "Deferred: {}", visitor.num_deferred());
                if let Some(sample_rate) = tail_params.clear_sample_rate {
                    if sample_rate != 0 && chunk_num % sample_rate == 0 {
                        info!(logger, #log::CHUNKING, "Clearing state after chunk {}", chunk_num);
                        visitor.clear_state(
                            &tail_params.clear_node_types,
                            &tail_params.clear_interned_types,
                        );
                    }
                }

                // Record checkpoint and update best_bounds
                if let Some((best_low, repo_high_bound)) = &mut best_bounds {
                    if tail_params.checkpoint_sample_rate != 0
                        && chunk_num % tail_params.checkpoint_sample_rate == 0
                    {
                        let new_best = best_low.map_or_else(
                            || Some(chunk_low),
                            |v| if chunk_low < v { Some(chunk_low) } else { None },
                        );
                        match (new_best, tail_params.checkpoints.as_ref()) {
                            (Some(new_best), Some(checkpoints)) => {
                                let now = Timestamp::now();
                                if let Some(checkpoint) = &mut checkpoint {
                                    checkpoint.lower_bound = new_best;
                                    checkpoint.upper_bound = *repo_high_bound;
                                    checkpoint.update_timestamp = now;
                                    info!(logger, #log::CHUNKING, "Chunk {} updating checkpoint to ({}, {})", chunk_num, new_best, repo_high_bound);
                                    checkpoints.update(repo_id, checkpoint).await?;
                                } else {
                                    let new_cp = Checkpoint {
                                        lower_bound: new_best,
                                        upper_bound: *repo_high_bound,
                                        create_timestamp: now,
                                        update_timestamp: now,
                                    };
                                    info!(logger, #log::CHUNKING, "Chunk {} inserting checkpoint ({}, {})", chunk_num, new_best, repo_high_bound);
                                    checkpoints.insert(repo_id, &new_cp).await?;
                                    checkpoint.replace(new_cp);
                                }
                                best_low.replace(new_best);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        visitor.end_chunks(contiguous_bounds)?;

        if let Some((chunk_size, _heads_fetcher)) = &chunk_params {
            info!(
                repo_params.logger, #log::CHUNKING,
                "Completed in {} chunks of size {}", chunk_num, chunk_size
            );
        };

        match tail_secs {
            Some(interval) => {
                let start = Instant::now();
                let next_iter_deadline = start + Duration::from_secs(interval);
                tokio::time::delay_until(next_iter_deadline).await;
                let age_secs = state_start.since_seconds();
                if age_secs >= 0 && Duration::from_secs(age_secs as u64) > tail_params.state_max_age
                {
                    // Walk state is too old, clear it.
                    info!(
                        repo_params.logger,
                        "Clearing walk state after {} seconds", age_secs
                    );
                    visitor
                        .clear_state(&NodeType::iter().collect(), &InternedType::iter().collect());
                    state_start = Timestamp::now();
                }
            }
            None => return Ok(()),
        }
    }
}
