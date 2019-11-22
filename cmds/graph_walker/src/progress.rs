/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::graph::{Node, NodeType};
use cloned::cloned;
use context::CoreContext;
use failure_ext::Error;
use futures::{
    future::{self},
    Future, Stream,
};
use slog::info;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

// Print some status update at most once per second, passing on all data unchanged
// TODO, could pass walk state if wanted aggregate to continue increasing on each tail step
pub fn progress_stream<InStream, ND>(
    ctx: CoreContext,
    sample_rate: usize,
    s: InStream,
) -> impl Stream<Item = (Node, Option<ND>), Error = Error>
where
    InStream: 'static + Stream<Item = (Node, Option<ND>), Error = Error> + Send,
{
    let min_duration = Duration::from_secs(1);

    let mut seen = 0;
    let mut loaded = 0;
    let mut last_update = Instant::now();
    let mut stats: HashMap<NodeType, (usize, usize)> = HashMap::new();

    s.map(move |(n, nd)| {
        seen += 1;
        let k = n.get_type();
        let data_count = match nd {
            None => 0,
            _ => 1,
        };
        loaded += data_count;
        match &mut stats.get_mut(&k) {
            None => {
                stats.insert(k, (1, data_count));
            }
            Some((seen, loaded)) => {
                *seen += 1;
                *loaded += data_count;
            }
        }
        if seen % sample_rate == 0 {
            let new_update = Instant::now();
            if (new_update - last_update) >= min_duration {
                info!(
                    ctx.logger(),
                    "Steps so far ({}, {}) {:?}", seen, loaded, stats
                );
                last_update = new_update;
            }
        }
        (n, nd)
    })
}

// Simple count.
pub fn do_count<InStream, ND>(
    ctx: CoreContext,
    s: InStream,
) -> impl Future<Item = (), Error = Error>
where
    InStream: Stream<Item = (Node, Option<ND>), Error = Error>,
{
    let init_stats: (usize, usize) = (0, 0);

    s.fold(init_stats, {
        move |(mut seen, mut loaded), (_n, nd)| {
            let data_count = match nd {
                None => 0,
                _ => 1,
            };
            seen += 1;
            loaded += data_count;
            future::ok::<_, Error>((seen, loaded))
        }
    })
    .map({
        cloned!(ctx);
        move |stats| {
            info!(ctx.logger(), "Final count: {:?}", stats);
            ()
        }
    })
}
