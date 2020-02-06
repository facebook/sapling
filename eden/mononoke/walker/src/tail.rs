/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::setup::{RepoWalkDatasources, RepoWalkParams};
use crate::walk::{walk_exact, WalkVisitor};

use anyhow::Error;
use cloned::cloned;
use context::CoreContext;

use futures_preview::{
    future::Future,
    stream::{repeat, BoxStream, StreamExt},
};

use tokio_preview::time::{Duration, Instant};

pub async fn walk_exact_tail<SinkFac, SinkOut, WS, VOut>(
    ctx: CoreContext,
    datasources: RepoWalkDatasources,
    walk_params: RepoWalkParams,
    walk_state: WS,
    make_sink: SinkFac,
) -> Result<(), Error>
where
    SinkFac: 'static + FnOnce(BoxStream<'static, Result<VOut, Error>>) -> SinkOut + Clone + Send,
    SinkOut: Future<Output = Result<(), Error>> + 'static + Send,
    WS: 'static + Clone + WalkVisitor<VOut> + Send,
    VOut: 'static + Send,
{
    let scuba_builder = datasources.scuba_builder;
    let repo = datasources.blobrepo.await?;
    let tail_secs = walk_params.tail_secs.clone();
    let mut stream: BoxStream<'static, Result<_, Error>> = repeat(())
        .then({
            move |_| {
                cloned!(ctx, repo, walk_state, make_sink,);
                {
                    let walk_output = walk_exact(
                        ctx,
                        repo,
                        walk_params.enable_derive,
                        walk_params.walk_roots.clone(),
                        walk_state,
                        walk_params.scheduled_max,
                        walk_params.error_as_data_node_types.clone(),
                        walk_params.error_as_data_edge_types.clone(),
                        scuba_builder.clone(),
                    );
                    make_sink(walk_output)
                }
            }
        })
        .boxed();
    match tail_secs {
        None => match stream.next().await {
            None => Ok(()),
            Some(r) => r,
        },
        Some(interval) => {
            stream
                .for_each(async move |_| {
                    let start = Instant::now();
                    let next_iter_deadline = start + Duration::from_secs(interval);
                    let _ = tokio_preview::time::delay_until(next_iter_deadline);
                })
                .await;
            Ok(())
        }
    }
}
