/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::graph::{FileContentData, Node, NodeData};
use crate::parse_args::parse_args_common;
use crate::progress::{do_count, progress_stream};
use crate::state::WalkStateArcMutex;
use crate::walk::walk_exact;

use clap::ArgMatches;
use cloned::cloned;
use context::CoreContext;
use failure_ext::Error;
use fbinit::FacebookInit;
use futures::{
    future::{self, loop_fn, Loop},
    Future, Stream,
};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use slog::Logger;
use std::time::{Duration, Instant};
use tokio_timer::Delay;

// Force load of leaf data like file contents that graph traversal did not need
pub fn loading_stream<InStream>(
    s: InStream,
) -> impl Stream<Item = (Node, Option<NodeData>), Error = Error>
where
    InStream: Stream<Item = (Node, Option<NodeData>), Error = Error>,
{
    s.map(move |(n, nd)| match nd {
        Some(d) => match d {
            NodeData::FileContent(FileContentData::ContentStream(file_bytes_stream)) => {
                file_bytes_stream
                    .fold(0, |acc, file_bytes| {
                        future::ok::<_, Error>(acc + file_bytes.size())
                    })
                    .map(|num_bytes| NodeData::FileContent(FileContentData::Consumed(num_bytes)))
                    .left_future()
            }
            _ => future::ok(d).right_future(),
        }
        .map(move |d| (n, Some(d)))
        .left_future(),
        None => future::ok((n, nd)).right_future(),
    })
    .buffered(100)
}

// Starts from the graph, (as opposed to walking from blobstore enumeration)
pub fn scrub_objects(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    let (blobrepo_fut, walk_params) =
        try_boxfuture!(parse_args_common(fb, &logger, matches, sub_m));
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    // Create this outside the loop so tail mode can reuse it
    let node_checker = WalkStateArcMutex::new();

    // Run a simple traversal
    let traversal_fut = blobrepo_fut.and_then(move |repo| {
        loop_fn((), move |()| {
            cloned!(ctx, repo, node_checker, walk_params,);
            let include_types = walk_params.include_types;
            let raw_stream = walk_exact(
                ctx.clone(),
                repo,
                walk_params.walk_roots,
                node_checker,
                move |walk_item| include_types.contains(&walk_item.get_type()),
                walk_params.scheduled_max,
            );
            let loading = loading_stream(raw_stream);
            let progress = progress_stream(ctx.clone(), 100, loading);
            let one_fut = do_count(ctx, progress);

            let tail_secs = walk_params.tail_secs;
            let next_fut = one_fut.and_then(move |_| match tail_secs {
                None => future::ok(Loop::Break(())).left_future(),
                Some(interval) => {
                    let start = Instant::now();
                    let next_iter_deadline = start + Duration::from_secs(interval);
                    Delay::new(next_iter_deadline)
                        .map_err(Error::from)
                        .and_then(|_| future::ok(Loop::Continue(())))
                        .right_future()
                }
            });

            next_fut
        })
    });

    traversal_fut.boxify()
}
