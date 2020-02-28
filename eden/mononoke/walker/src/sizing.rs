/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{FileContentData, Node, NodeData};
use crate::progress::{
    progress_stream, report_state, ProgressRecorderUnprotected, ProgressReporterUnprotected,
    ProgressStateMutex,
};
use crate::setup::{setup_common, COMPRESSION_BENEFIT, COMPRESSION_LEVEL_ARG, SAMPLE_RATE_ARG};
use crate::state::{WalkState, WalkStateCHashMap};
use crate::tail::{walk_exact_tail, RepoWalkRun};

use anyhow::{format_err, Error};
use async_compression::{metered::MeteredWrite, Compressor, CompressorType};
use clap::ArgMatches;
use cloned::cloned;
use cmdlib::args;
use fbinit::FacebookInit;
use futures_preview::{
    future::{self, BoxFuture, FutureExt, TryFutureExt},
    stream::{Stream, TryStreamExt},
};
use mercurial_types::FileBytes;
use scuba_ext::ScubaSampleBuilder;
use slog::{info, Logger};
use std::{
    cmp::min,
    io::{Cursor, Write},
    ops::Add,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Default, Debug)]
struct SizingStats {
    raw: usize,
    compressed: usize,
}

impl SizingStats {
    fn compression_benefit_pct(&self) -> usize {
        if self.raw == 0 {
            0
        } else {
            100 * (self.raw - self.compressed) / self.raw
        }
    }
}

impl Add<SizingStats> for SizingStats {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            raw: self.raw + other.raw,
            compressed: self.compressed + other.compressed,
        }
    }
}

fn get_sizes(file_bytes: FileBytes, compressor_type: CompressorType) -> Result<SizingStats, Error> {
    let raw = file_bytes.size();
    let compressed_buf = MeteredWrite::new(Cursor::new(Vec::with_capacity(4 * 1024)));
    let mut compressor = Compressor::new(compressed_buf, compressor_type);
    compressor
        .write_all(file_bytes.as_bytes())
        .map_err(|e| Error::from(e))?;
    let compressed_buf = compressor
        .try_finish()
        .map_err(|(_encoder, e)| Error::from(e))?;
    // Assume we wouldn't compress if its bigger
    let compressed = min(raw, compressed_buf.total_thru() as usize);
    Ok(SizingStats { raw, compressed })
}

// Force load of leaf data and check compression ratio
fn size_sampling_stream<InStream, InStats>(
    sample_rate: u64,
    scheduled_max: usize,
    s: InStream,
    compressor_type: CompressorType,
) -> impl Stream<Item = Result<(Node, Option<NodeData>, Option<SizingStats>), Error>>
where
    InStream:
        Stream<Item = Result<(Node, Option<NodeData>, Option<InStats>), Error>> + 'static + Send,
    InStats: 'static + Send,
{
    s.map_ok(move |(n, data_opt, _stats_opt)| match (&n, data_opt) {
        // Sample on first byte of hash for reproducible values
        (Node::FileContent(content_id), Some(NodeData::FileContent(fc)))
            if content_id.blake2().as_ref()[0] as u64 % sample_rate == 0 =>
        {
            match fc {
                FileContentData::Consumed(_num_loaded_bytes) => future::err(format_err!(
                    "Stream was consumed before compression estimate"
                ))
                .left_future(),
                FileContentData::ContentStream(file_bytes_stream) => file_bytes_stream
                    .try_fold(SizingStats::default(), move |acc, file_bytes| {
                        future::ready(
                            get_sizes(file_bytes, compressor_type).map(|sizes| acc + sizes),
                        )
                    })
                    .right_future(),
            }
            .map_ok(move |sizes| {
                (
                    n,
                    Some(NodeData::FileContent(FileContentData::Consumed(sizes.raw))),
                    Some(sizes),
                )
            })
            .left_future()
        }
        (_, data_opt) => future::ok((n, data_opt, None)).right_future(),
    })
    .try_buffer_unordered(scheduled_max)
}

struct SizingState {
    logger: Logger,
    sample: SizingStats,
    total: SizingStats,
    num_sampled: u64,
    throttle_sample_rate: u64,
    throttle_duration: Duration,
    last_update: Instant,
}

impl SizingState {
    pub fn new(logger: Logger, sample_rate: u64) -> Self {
        let now = Instant::now();
        Self {
            logger,
            sample: SizingStats::default(),
            total: SizingStats::default(),
            num_sampled: 0,
            throttle_sample_rate: sample_rate,
            throttle_duration: Duration::from_secs(1),
            last_update: now,
        }
    }
}

impl ProgressRecorderUnprotected<SizingStats> for SizingState {
    fn record_step(self: &mut Self, _n: &Node, opt: Option<&SizingStats>) {
        match opt {
            Some(file_stats) => {
                self.num_sampled += 1;
                self.total = self.total + *file_stats;
                self.sample = *file_stats;
            }
            None => {}
        }
    }

    fn set_sample_builder(&mut self, _s: ScubaSampleBuilder) {
        ()
    }
}

impl ProgressReporterUnprotected for SizingState {
    // For size sampling we report via glog
    fn report_progress(self: &mut Self) {
        info!(
            self.logger,
            "Samples={}, Raw,Compressed,%OfRaw; Total: {:?},{:03}% File: {:?},{:03}%",
            self.num_sampled,
            self.total,
            self.total.compression_benefit_pct(),
            self.sample,
            self.sample.compression_benefit_pct()
        );
    }

    // Drive the report sampling by the number of files we have tried compressing
    fn report_throttled(self: &mut Self) -> Option<Duration> {
        if self.num_sampled % self.throttle_sample_rate == 0 {
            let new_update = Instant::now();
            let delta_time = new_update.duration_since(self.last_update);
            if delta_time >= self.throttle_duration {
                self.report_progress();
                self.last_update = new_update;
            }
            Some(delta_time)
        } else {
            None
        }
    }
}

// Subcommand entry point for estimate of file compression benefit
pub fn compression_benefit(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<'static, Result<(), Error>> {
    match setup_common(COMPRESSION_BENEFIT, fb, &logger, matches, sub_m) {
        Err(e) => future::err::<_, Error>(e).boxed(),
        Ok((datasources, walk_params)) => {
            let sizing_state = ProgressStateMutex::new(SizingState::new(logger.clone(), 1));
            let compression_level = args::get_i32_opt(&sub_m, COMPRESSION_LEVEL_ARG).unwrap_or(3);
            let sample_rate = args::get_u64_opt(&sub_m, SAMPLE_RATE_ARG).unwrap_or(100);
            cloned!(
                walk_params.progress_state,
                walk_params.quiet,
                walk_params.scheduled_max
            );
            let make_sink = move |run: RepoWalkRun| {
                cloned!(run.ctx);
                async move |walk_output| {
                    cloned!(ctx, sizing_state);
                    let walk_progress =
                        progress_stream(quiet, &progress_state.clone(), walk_output);
                    let compressor = size_sampling_stream(
                        sample_rate,
                        scheduled_max,
                        walk_progress,
                        CompressorType::Zstd {
                            level: compression_level,
                        },
                    );
                    let report_sizing = progress_stream(quiet, &sizing_state.clone(), compressor);
                    report_state(ctx, sizing_state, report_sizing).await
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
            walk_exact_tail(fb, logger, datasources, walk_params, walk_state, make_sink).boxed()
        }
    }
}
