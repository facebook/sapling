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
use crate::state::WalkState;
use crate::tail::walk_exact_tail;
use crate::walk::StepStats;

use anyhow::Error;
use clap::ArgMatches;
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    future::{self},
    Future, Stream,
};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use slog::Logger;

// Force load of leaf data like file contents that graph traversal did not need
pub fn loading_stream<InStream>(
    s: InStream,
) -> impl Stream<Item = (Node, Option<(StepStats, NodeData)>), Error = Error>
where
    InStream: Stream<Item = (Node, Option<(StepStats, NodeData)>), Error = Error>,
{
    s.map(move |(n, opt)| match opt {
        Some((ss, nd)) => match nd {
            NodeData::FileContent(FileContentData::ContentStream(file_bytes_stream)) => {
                file_bytes_stream
                    .fold(0, |acc, file_bytes| {
                        future::ok::<_, Error>(acc + file_bytes.size())
                    })
                    .map(|num_bytes| NodeData::FileContent(FileContentData::Consumed(num_bytes)))
                    .left_future()
            }
            _ => future::ok(nd).right_future(),
        }
        .map(move |d| (n, Some((ss, d))))
        .left_future(),
        None => future::ok((n, opt)).right_future(),
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
    let (blobrepo, walk_params) = try_boxfuture!(parse_args_common(fb, &logger, matches, sub_m));
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let make_sink = {
        cloned!(ctx);
        move |walk_output| {
            cloned!(ctx);
            let loading = loading_stream(walk_output);
            let show_progress = progress_stream(ctx.clone(), 100, loading);
            let one_fut = do_count(ctx, show_progress);
            one_fut
        }
    };
    let walk_state = WalkState::new();
    walk_exact_tail(ctx, walk_params, walk_state, blobrepo, make_sink).boxify()
}
