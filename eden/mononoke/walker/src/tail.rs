/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::setup::{RepoWalkDatasources, RepoWalkParams};
use crate::walk::{walk_exact, WalkVisitor};

use anyhow::Error;
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{future::Future, stream::BoxStream};
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use tokio_preview::time::{Duration, Instant};

#[derive(Clone)]
pub struct RepoWalkRun {
    pub ctx: CoreContext,
    pub scuba_builder: ScubaSampleBuilder,
}

pub async fn walk_exact_tail<RunFac, SinkFac, SinkOut, WS, VOut, Route>(
    fb: FacebookInit,
    logger: Logger,
    datasources: RepoWalkDatasources,
    walk_params: RepoWalkParams,
    walk_state: WS,
    make_run: RunFac,
) -> Result<(), Error>
where
    RunFac: 'static + Clone + Send + Sync + FnOnce(RepoWalkRun) -> SinkFac,
    SinkFac: 'static + FnOnce(BoxStream<'static, Result<VOut, Error>>) -> SinkOut + Clone + Send,
    SinkOut: Future<Output = Result<(), Error>> + 'static + Send,
    WS: 'static + Clone + WalkVisitor<VOut, Route> + Send,
    VOut: 'static + Send,
    Route: 'static + Send + Clone,
{
    let scuba_builder = datasources.scuba_builder;
    let repo = datasources.blobrepo.await?;
    let tail_secs = walk_params.tail_secs.clone();
    loop {
        cloned!(make_run, repo, mut scuba_builder, walk_state,);

        let ctx = CoreContext::new_with_logger(fb, logger.clone());
        scuba_builder.add("session", ctx.session().session_id().to_string());
        let walk_run = RepoWalkRun {
            ctx: ctx.clone(),
            scuba_builder: scuba_builder.clone(),
        };

        let walk_output = walk_exact(
            ctx,
            repo,
            walk_params.enable_derive,
            walk_params.walk_roots.clone(),
            walk_state,
            walk_params.scheduled_max,
            walk_params.error_as_data_node_types.clone(),
            walk_params.error_as_data_edge_types.clone(),
            scuba_builder,
        );

        let make_sink = make_run(walk_run);
        make_sink(walk_output).await?;

        match tail_secs {
            Some(interval) => {
                let start = Instant::now();
                let next_iter_deadline = start + Duration::from_secs(interval);
                tokio_preview::time::delay_until(next_iter_deadline).await;
            }
            None => return Ok(()),
        }
    }
}
