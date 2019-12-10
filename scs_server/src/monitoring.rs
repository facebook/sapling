/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use futures::future::Future;
use futures::stream::Stream;
use futures_ext::{BoxFuture, FutureExt};
use futures_preview::FutureExt as NewFutureExt;
use futures_util::try_future::TryFutureExt;
use mononoke_api::{CoreContext, Mononoke};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SUBMIT_STATS_ONCE_PER_SECS: u64 = 10;
const WAIT_UNTIL_SUBMIT_FIRST_STAT_SECS: u64 = 0;

pub fn monitoring_stats_submitter(
    ctx: CoreContext,
    mononoke: Arc<Mononoke>,
) -> BoxFuture<(), Error> {
    let at = Instant::now() + Duration::from_secs(WAIT_UNTIL_SUBMIT_FIRST_STAT_SECS);
    let interval = Duration::from_secs(SUBMIT_STATS_ONCE_PER_SECS);
    let reporter = move || {
        let mononoke_clone = mononoke.clone();
        let ctx_clone = ctx.clone();
        async move { mononoke_clone.report_monitoring_stats(ctx_clone).await }
            .boxed()
            .compat()
            .map_err(Error::from)
            .boxify()
    };
    schedule_fn_on_stream(
        tokio::timer::Interval::new(at, interval).map_err(Error::from),
        reporter,
    )
}

// Schedule a given function to execute once per item of the given stream
fn schedule_fn_on_stream<S, F>(
    stream: S,
    f: impl Fn() -> F + Send + Sync + 'static,
) -> BoxFuture<(), Error>
where
    S: Stream<Error = Error> + Send + 'static,
    F: Future<Error = Error> + Send + 'static,
{
    stream
        .for_each(move |_| f().map(|_| ()).map_err(Error::from))
        .boxify()
}
