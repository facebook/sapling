/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{FileContentData, Node, NodeData};
use crate::progress::{progress_stream, report_state};
use crate::setup::{setup_common, LIMIT_DATA_FETCH_ARG, SCRUB};
use crate::state::{WalkState, WalkStateCHashMap};
use crate::tail::walk_exact_tail;

use anyhow::Error;
use clap::ArgMatches;
use context::CoreContext;
use fbinit::FacebookInit;
use futures_preview::{
    future::{self, BoxFuture, FutureExt},
    stream::{Stream, TryStreamExt},
    TryFutureExt,
};
use slog::Logger;

// Force load of leaf data like file contents that graph traversal did not need
pub fn loading_stream<InStream, SS>(
    limit_data_fetch: bool,
    scheduled_max: usize,
    s: InStream,
) -> impl Stream<Item = Result<(Node, Option<NodeData>, Option<SS>), Error>>
where
    InStream: Stream<Item = Result<(Node, Option<NodeData>, Option<SS>), Error>> + 'static + Send,
{
    s.map_ok(move |(n, nd, ss)| match nd {
        Some(NodeData::FileContent(FileContentData::ContentStream(file_bytes_stream)))
            if !limit_data_fetch =>
        {
            file_bytes_stream
                .try_fold(0, |acc, file_bytes| future::ok(acc + file_bytes.size()))
                .map_ok(|bytes| {
                    (
                        n,
                        Some(NodeData::FileContent(FileContentData::Consumed(bytes))),
                        ss,
                    )
                })
                .left_future()
        }
        _ => future::ok((n, nd, ss)).right_future(),
    })
    .try_buffer_unordered(scheduled_max)
}

// Starts from the graph, (as opposed to walking from blobstore enumeration)
pub fn scrub_objects(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<'static, Result<(), Error>> {
    match setup_common(SCRUB, fb, &logger, matches, sub_m) {
        Err(e) => future::err::<_, Error>(e).boxed(),
        Ok((datasources, walk_params)) => {
            let ctx = CoreContext::new_with_logger(fb, logger.clone());
            let limit_data_fetch = sub_m.is_present(LIMIT_DATA_FETCH_ARG);

            let make_sink = {
                let scheduled_max = walk_params.scheduled_max;
                let quiet = walk_params.quiet;
                let progress_state = walk_params.progress_state.clone();
                let ctx = ctx.clone();
                async move |walk_output| {
                    let loading = loading_stream(limit_data_fetch, scheduled_max, walk_output);
                    let show_progress = progress_stream(quiet, &progress_state, loading);
                    report_state(ctx, progress_state, show_progress).await
                }
            };

            let walk_state = WalkState::new(WalkStateCHashMap::new(
                walk_params.include_node_types.clone(),
                walk_params.include_edge_types.clone(),
            ));
            walk_exact_tail(ctx, datasources, walk_params, walk_state, make_sink).boxed()
        }
    }
}
