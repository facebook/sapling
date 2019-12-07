/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::parse_args::parse_args_common;
use crate::progress::{do_count, progress_stream};
use crate::state::WalkState;
use crate::tail::walk_exact_tail;

use anyhow::Error;
use clap::ArgMatches;
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use slog::Logger;

// Starts from the graph, (as opposed to walking from blobstore enumeration)
pub fn count_objects(
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
            let show_progress = progress_stream(ctx.clone(), 1000, walk_output);
            let one_fut = do_count(ctx, show_progress);
            one_fut
        }
    };
    let walk_state = WalkState::new();
    walk_exact_tail(ctx, walk_params, walk_state, blobrepo, make_sink).boxify()
}
