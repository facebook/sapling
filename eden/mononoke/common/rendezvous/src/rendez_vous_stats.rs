/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use stats::prelude::*;

define_stats_struct! {
    RendezVousStats("mononoke.rdv.{}", label: String),

    dispatch_no_batch: timeseries("dispatch_no_batch"; Sum),
    dispatch_batch_early: timeseries("dispatch_batch_early"; Sum),
    dispatch_batch_scheduled: timeseries("dispatch_batch_scheduled"; Sum),

    keys_dispatched: timeseries("keys_dispatched"; Average, Sum),
    keys_deduplicated: timeseries("keys_deduplicated"; Sum),

    fetch_completion_time_ms: histogram(1, 0, 50, Average; P 50; P 95; P 99),

    inflight: singleton_counter("inflight"),
}
