/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::setup::JobWalkParams;
use crate::walk::{walk_exact, RepoWalkParams, StepRoute, TypeWalkParams, WalkVisitor};

use anyhow::Error;
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{future::Future, stream::BoxStream};
use slog::Logger;
use tokio::time::{Duration, Instant};

pub async fn walk_exact_tail<RunFac, SinkFac, SinkOut, V, VOut, Route>(
    fb: FacebookInit,
    logger: Logger,
    job_params: JobWalkParams,
    mut repo_params: RepoWalkParams,
    type_params: TypeWalkParams,
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
        let ctx = CoreContext::new_with_logger(fb, logger.clone());
        let session_text = ctx.session().metadata().session_id().to_string();
        repo_params.scuba_builder.add("session", session_text);

        let walk_output = {
            cloned!(ctx, repo_params);
            walk_exact(ctx, visitor, job_params, repo_params, type_params)
        };

        let make_sink = make_run(&ctx, &repo_params);
        make_sink(walk_output).await?;

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
