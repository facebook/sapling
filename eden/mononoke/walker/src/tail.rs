/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{ChangesetKey, Node, NodeType};
use crate::log;
use crate::setup::JobWalkParams;
use crate::walk::{
    walk_exact, OutgoingEdge, RepoWalkParams, RepoWalkTypeParams, StepRoute, WalkVisitor,
};

use anyhow::{bail, Error};
use bulkops::{Direction, PublicChangesetBulkFetch, MAX_FETCH_STEP, MIN_FETCH_STEP};
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    future::{self, Future},
    stream::{self, BoxStream, StreamExt, TryStreamExt},
};
use mononoke_types::ChangesetId;
use slog::info;
use std::collections::HashSet;
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

pub async fn walk_exact_tail<RunFac, SinkFac, SinkOut, V, VOut, Route>(
    fb: FacebookInit,
    job_params: JobWalkParams,
    mut repo_params: RepoWalkParams,
    type_params: RepoWalkTypeParams,
    public_changeset_chunk_by: HashSet<NodeType>,
    visitor: V,
    make_run: RunFac,
) -> Result<(), Error>
where
    RunFac: 'static + Clone + Send + Sync + FnOnce(&CoreContext, &RepoWalkParams) -> SinkFac,
    SinkFac: 'static + FnOnce(BoxStream<'static, Result<VOut, Error>>) -> SinkOut + Clone + Send,
    SinkOut: Future<Output = Result<(), Error>> + 'static + Send,
    V: 'static + Clone + WalkVisitor<VOut, Route> + Send + Sync,
    VOut: 'static + Send,
    Route: 'static + Send + Clone + StepRoute,
{
    loop {
        cloned!(job_params, type_params, make_run, visitor);
        let tail_secs = job_params.tail_secs;
        // Each loop get new ctx and thus session id so we can distinguish runs
        let ctx = CoreContext::new_with_logger(fb, repo_params.logger.clone());
        let session_text = ctx.session().metadata().session_id().to_string();
        repo_params.scuba_builder.add("session", session_text);

        let chunk_params = job_params
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

        // Done in separate step so that heads_fetcher stays live in chunk_params
        let chunk_stream = if let Some((chunk_size, heads_fetcher)) = &chunk_params {
            heads_fetcher
                .fetch_ids(&ctx, Direction::NewestFirst, None)
                // TODO(ahornby) use chunk bounds for checkpointing
                .map_ok(|(cs_id, _chunk_bounds)| cs_id)
                .chunks(*chunk_size)
                .map(move |v| v.into_iter().collect::<Result<HashSet<_>, Error>>())
                .left_stream()
        } else {
            stream::once(future::ok(HashSet::new())).right_stream()
        };

        let chunk_count: usize = chunk_stream
            .map(|chunk_members| {
                let chunk_members = chunk_members?;
                cloned!(mut repo_params);
                let extra_roots = visitor.start_chunk(&chunk_members)?.into_iter();
                let chunk_roots = roots_for_chunk(chunk_members, &public_changeset_chunk_by)?;
                repo_params.walk_roots.extend(chunk_roots);
                repo_params.walk_roots.extend(extra_roots);
                Ok(repo_params)
            })
            .and_then(|repo_params| {
                cloned!(ctx, job_params, make_run, type_params, visitor);
                let make_sink = make_run(&ctx, &repo_params);
                let logger = repo_params.logger.clone();
                let walk_output =
                    walk_exact(ctx, visitor.clone(), job_params, repo_params, type_params);
                async move {
                    let res = make_sink(walk_output).await?;
                    if is_chunking {
                        info!(logger, #log::LOADED, "Deferred: {}", visitor.num_deferred());
                    }
                    Ok::<_, Error>(res)
                }
            })
            .try_fold(0, |acc, _| future::ok(acc + 1))
            .await?;

        visitor.end_chunks()?;

        if let Some((chunk_size, _heads_fetcher)) = &chunk_params {
            info!(
                repo_params.logger, #log::LOADED,
                "Completed in {} chunks of {}", chunk_count, chunk_size
            );
        };

        match tail_secs {
            Some(interval) => {
                let start = Instant::now();
                let next_iter_deadline = start + Duration::from_secs(interval);
                tokio::time::delay_until(next_iter_deadline).await;
            }
            None => return Ok(()),
        }
    }
}
