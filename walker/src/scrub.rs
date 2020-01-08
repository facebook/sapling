/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::graph::{FileContentData, Node, NodeData};
use crate::progress::{
    progress_stream, report_state, ProgressStateCountByType, ProgressStateMutex,
};
use crate::setup::{setup_common, LIMIT_DATA_FETCH_ARG, SCRUB};
use crate::state::{WalkState, WalkStateCHashMap};
use crate::tail::walk_exact_tail;

use anyhow::Error;
use clap::ArgMatches;
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    future::{self},
    Future, Stream,
};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use slog::Logger;
use std::time::Duration;

const PROGRESS_SAMPLE_RATE: u64 = 100;
const PROGRESS_SAMPLE_DURATION_S: u64 = 1;

// Force load of leaf data like file contents that graph traversal did not need
pub fn loading_stream<InStream, SS>(
    limit_data_fetch: bool,
    scheduled_max: usize,
    s: InStream,
) -> impl Stream<Item = (Node, Option<NodeData>, Option<SS>), Error = Error>
where
    InStream: Stream<Item = (Node, Option<NodeData>, Option<SS>), Error = Error>,
{
    s.map(move |(n, nd, ss)| match nd {
        Some(NodeData::FileContent(FileContentData::ContentStream(file_bytes_stream)))
            if !limit_data_fetch =>
        {
            file_bytes_stream
                .fold(0, |acc, file_bytes| {
                    future::ok::<_, Error>(acc + file_bytes.size())
                })
                .map(|num_bytes| {
                    (
                        n,
                        Some(NodeData::FileContent(FileContentData::Consumed(num_bytes))),
                        ss,
                    )
                })
                .left_future()
        }
        _ => future::ok((n, nd, ss)).right_future(),
    })
    .buffer_unordered(scheduled_max)
}

// Starts from the graph, (as opposed to walking from blobstore enumeration)
pub fn scrub_objects(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    let (datasources, walk_params) =
        try_boxfuture!(setup_common(SCRUB, fb, &logger, matches, sub_m));
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let repo_stats_key = try_boxfuture!(args::get_repo_name(fb, &matches));
    let progress_state = ProgressStateMutex::new(ProgressStateCountByType::new(
        logger.clone(),
        SCRUB,
        repo_stats_key.clone(),
        walk_params.progress_node_types(),
        PROGRESS_SAMPLE_RATE,
        Duration::from_secs(PROGRESS_SAMPLE_DURATION_S),
    ));

    let limit_data_fetch = sub_m.is_present(LIMIT_DATA_FETCH_ARG);

    let make_sink = {
        cloned!(ctx, walk_params.quiet, walk_params.scheduled_max);
        move |walk_output| {
            cloned!(ctx, progress_state);
            let loading = loading_stream(limit_data_fetch, scheduled_max, walk_output);
            let show_progress = progress_stream(quiet, progress_state.clone(), loading);
            let one_fut = report_state(ctx, progress_state.clone(), show_progress);
            one_fut
        }
    };
    cloned!(
        walk_params.include_node_types,
        walk_params.include_edge_types
    );
    let walk_state = WalkState::new(WalkStateCHashMap::new(
        include_node_types,
        include_edge_types,
    ));
    walk_exact_tail(ctx, datasources, walk_params, walk_state, make_sink).boxify()
}
