/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::Relaxed;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use clidispatch::ReqCtx;
use cliparser::define_flags;
use progress_model::IoSample;
use progress_model::IoTimeSeries;
use progress_model::ProgressBar;
use progress_model::Registry;

use super::ConfigSet;
use super::Result;
use super::IO;

define_flags! {
    pub struct DebugRacyOutputOpts {
        /// number of time series to show
        time_series: i64 = 1,

        /// number of progress bars to show
        progress_bars: i64 = 10,

        /// total for a progress bar
        progress_total: i64 = 500,

        /// max update interval for a progress bar, in milliseconds
        progress_interval_ms: i64 = 200,

        /// total outputs
        output_total: i64 = 100,

        /// max interval, in milliseconds for outputs
        output_interval_ms: i64 = 1000,
    }
}

pub fn run(ctx: ReqCtx<DebugRacyOutputOpts>, _config: &mut ConfigSet) -> Result<u8> {
    add_time_series(ctx.opts.time_series as _);
    add_progress_bar_threads(
        ctx.opts.progress_bars as _,
        ctx.opts.progress_total as _,
        ctx.opts.progress_interval_ms as _,
    );
    write_random_outputs(
        ctx.io(),
        ctx.opts.output_total as _,
        ctx.opts.output_interval_ms as _,
    );
    Ok(0)
}

fn add_time_series(count: usize) {
    let registry = Registry::main();
    for _ in 0..count {
        let time_series = IoTimeSeries::new("Metric", "items");
        let take_sample = {
            || {
                static TOTAL: AtomicU64 = AtomicU64::new(0);
                let v = TOTAL.fetch_add(rand::random::<u64>() % 0xffff, Relaxed);
                IoSample::from_io_bytes_count(v, v, v)
            }
        };
        let task = time_series.async_sampling(take_sample, IoTimeSeries::default_sample_interval());
        async_runtime::spawn(task);
        registry.register_io_time_series(&time_series);
    }
}

fn add_progress_bar_threads(
    num_threads: usize,
    total: u64,
    interval_ms: u64,
) -> Vec<JoinHandle<()>> {
    let threads: Vec<JoinHandle<_>> = (0..num_threads)
        .map(|_| {
            let bar = ProgressBar::register_new("Progress", total, "items");
            thread::spawn(move || {
                for _i in 0..total {
                    bar.increase_position(1);
                    sleep_random_ms(interval_ms);
                }
            })
        })
        .collect();
    threads
}

fn write_random_outputs(io: &IO, total: usize, interval_ms: u64) {
    let mut write = {
        let mut out = io.output();
        let mut err = io.error();
        move |use_out: bool, eol: bool, i: usize| {
            let msg = format!(
                "{}.{}{}",
                if use_out { "out" } else { "err" },
                i,
                if eol { ".\n" } else { "," }
            );
            let _ = if use_out {
                out.write(msg.as_bytes())
            } else {
                err.write(msg.as_bytes())
            };
        }
    };
    for i in 0..total {
        let use_out = rand::random();
        write(use_out, false, i);
        sleep_random_ms(5);
        write(use_out, true, i);
        sleep_random_ms(interval_ms);
    }
}

fn sleep_random_ms(max_ms: u64) {
    thread::sleep(Duration::from_millis(rand::random::<u64>() % max_ms));
}

pub fn aliases() -> &'static str {
    "debugracyoutput"
}

pub fn doc() -> &'static str {
    "exercise racy stdout / stderr / progress outputs"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
