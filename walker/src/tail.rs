/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::setup::RepoWalkParams;
use crate::walk::{walk_exact, WalkVisitor};

use anyhow::Error;
use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use futures::{
    stream::{repeat, Stream},
    Future,
};
use futures_ext::{BoxStream, FutureExt};
use std::time::{Duration, Instant};
use tokio_timer::Delay;

pub fn walk_exact_tail<SinkFac, SinkOut, WS, VOut>(
    ctx: CoreContext,
    walk_params: RepoWalkParams,
    walk_state: WS,
    blobrepo: impl Future<Item = BlobRepo, Error = Error>,
    make_sink: SinkFac,
) -> impl Future<Item = (), Error = Error>
where
    SinkFac: 'static + Fn(BoxStream<VOut, Error>) -> SinkOut,
    SinkOut: Future<Item = (), Error = Error>,
    WS: 'static + Clone + WalkVisitor<VOut> + Send,
    VOut: 'static + Send,
{
    let traversal_fut = blobrepo.and_then(move |repo| {
        cloned!(walk_params.tail_secs);
        let stream = repeat(()).and_then({
            move |()| {
                cloned!(ctx, repo, walk_params.walk_roots, walk_state,);
                let walk_output =
                    walk_exact(ctx, repo, walk_roots, walk_state, walk_params.scheduled_max);
                make_sink(walk_output)
            }
        });
        match tail_secs {
            // NOTE: This would be a lot nicer with async / await since could just .next().await
            None => stream
                .into_future()
                .map(|_| ())
                .map_err(|(e, _)| e)
                .left_future(),
            Some(interval) => stream
                .for_each(move |_| {
                    let start = Instant::now();
                    let next_iter_deadline = start + Duration::from_secs(interval);
                    Delay::new(next_iter_deadline).from_err()
                })
                .right_future(),
        }
    });
    traversal_fut
}
