/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::parse_args::parse_args_common;
use crate::progress::{
    progress_stream, report_state, ProgressStateCountByType, ProgressStateMutex,
};
use crate::state::{WalkState, WalkStateCHashMap};
use crate::tail::walk_exact_tail;

use anyhow::Error;
use clap::ArgMatches;
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use slog::Logger;
use std::time::Duration;

const PROGRESS_SAMPLE_RATE: u64 = 100;
const PROGRESS_SAMPLE_DURATION_S: u64 = 1;

// Starts from the graph, (as opposed to walking from blobstore enumeration)
pub fn count_objects(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    let (blobrepo, walk_params) = try_boxfuture!(parse_args_common(fb, &logger, matches, sub_m));
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let progress_state = ProgressStateMutex::new(ProgressStateCountByType::new(
        walk_params.progress_node_types(),
        PROGRESS_SAMPLE_RATE,
        Duration::from_secs(PROGRESS_SAMPLE_DURATION_S),
    ));
    let make_sink = {
        cloned!(ctx, walk_params.quiet);
        move |walk_output| {
            cloned!(ctx, progress_state);
            let show_progress =
                progress_stream(ctx.clone(), quiet, progress_state.clone(), walk_output);
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
    walk_exact_tail(ctx, walk_params, walk_state, blobrepo, make_sink).boxify()
}
