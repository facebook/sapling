/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{FileContentData, Node, NodeData, NodeType};
use crate::progress::{
    progress_stream, report_state, ProgressRecorderUnprotected, ProgressReporterUnprotected,
    ProgressStateMutex,
};
use crate::sampling::{NodeSamplingHandler, SamplingWalkVisitor};
use crate::setup::{
    parse_node_types, setup_common, COMPRESSION_BENEFIT, COMPRESSION_LEVEL_ARG,
    EXCLUDE_SAMPLE_NODE_TYPE_ARG, INCLUDE_SAMPLE_NODE_TYPE_ARG, SAMPLE_RATE_ARG,
};
use crate::state::WalkState;
use crate::tail::{walk_exact_tail, RepoWalkRun};

use anyhow::Error;
use async_compression::{metered::MeteredWrite, Compressor, CompressorType};
use bytes::Bytes;
use clap::ArgMatches;
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use derive_more::Add;
use fbinit::FacebookInit;
use futures::{
    future::{self, BoxFuture, FutureExt, TryFutureExt},
    stream::{Stream, TryStreamExt},
};
use mononoke_types::BlobstoreBytes;
use samplingblob::SamplingHandler;
use scuba_ext::ScubaSampleBuilder;
use slog::{info, Logger};
use std::{
    cmp::min,
    collections::HashMap,
    io::{Cursor, Write},
    sync::Arc,
    time::{Duration, Instant},
};

const DEFAULT_SAMPLING_NODE_TYPES: &[NodeType] = &[NodeType::FileContent];

#[derive(Add, Clone, Copy, Default, Debug)]
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

fn try_compress(raw_data: &Bytes, compressor_type: CompressorType) -> Result<SizingStats, Error> {
    let raw = raw_data.len();
    let compressed_buf = MeteredWrite::new(Cursor::new(Vec::with_capacity(4 * 1024)));
    let mut compressor = Compressor::new(compressed_buf, compressor_type);
    compressor.write_all(raw_data)?;
    let compressed_buf = compressor.try_finish().map_err(|(_encoder, e)| e)?;
    // Assume we wouldn't compress if its bigger
    let compressed = min(raw, compressed_buf.total_thru() as usize);
    Ok(SizingStats { raw, compressed })
}

// Force load of leaf data and check compression ratio
fn size_sampling_stream<InStream, InStats>(
    scheduled_max: usize,
    s: InStream,
    compressor_type: CompressorType,
    sampler: Arc<NodeSamplingHandler<SizingSample>>,
) -> impl Stream<Item = Result<(Node, Option<NodeData>, Option<SizingStats>), Error>>
where
    InStream:
        Stream<Item = Result<(Node, Option<NodeData>, Option<InStats>), Error>> + 'static + Send,
    InStats: 'static + Send,
{
    s.map_ok(move |(n, data_opt, _stats_opt)| match (&n, data_opt) {
        (Node::FileContent(_content_id), Some(NodeData::FileContent(fc)))
            if sampler.is_sampling(&n) =>
        {
            match fc {
                FileContentData::Consumed(_num_loaded_bytes) => {
                    future::ok(_num_loaded_bytes).left_future()
                }
                // Consume the stream to make sure we loaded all blobs
                FileContentData::ContentStream(file_bytes_stream) => file_bytes_stream
                    .try_fold(0, |acc, file_bytes| future::ok(acc + file_bytes.size()))
                    .right_future(),
            }
            .and_then({
                cloned!(sampler);
                move |fs_stream_size| {
                    // Report the blobstore sizes in sizing stats, more accurate than stream sizes, as headers included
                    let sizes = sampler
                        .complete_node(&n)
                        .map(|sizing_sample| {
                            sizing_sample.data.values().try_fold(
                                SizingStats::default(),
                                |acc, v| {
                                    try_compress(v.as_bytes(), compressor_type)
                                        .map(|sizes| acc + sizes)
                                },
                            )
                        })
                        .transpose();

                    future::ready(sizes.map(|sizes| {
                        // Report the filestore stream's bytes size in the Consumed node
                        (
                            n,
                            Some(NodeData::FileContent(FileContentData::Consumed(
                                fs_stream_size,
                            ))),
                            sizes,
                        )
                    }))
                }
            })
            .left_future()
        }
        (_, data_opt) => {
            // Free the memory
            sampler.complete_node(&n);
            future::ok((n, data_opt, None)).right_future()
        }
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
        if let Some(file_stats) = opt {
            self.num_sampled += 1;
            self.total = self.total + *file_stats;
            self.sample = *file_stats;
        }
    }

    fn set_sample_builder(&mut self, _s: ScubaSampleBuilder) {}
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

#[derive(Debug)]
struct SizingSample {
    data: HashMap<String, BlobstoreBytes>,
}

impl Default for SizingSample {
    fn default() -> Self {
        Self {
            data: HashMap::with_capacity(1),
        }
    }
}

impl SamplingHandler for NodeSamplingHandler<SizingSample> {
    fn sample_get(&self, ctx: CoreContext, key: String, value: Option<&BlobstoreBytes>) {
        ctx.sampling_key().map(|sampling_key| {
            self.inflight()
                .get_mut(sampling_key)
                .map(|mut guard| value.map(|value| guard.data.insert(key.clone(), value.clone())))
        });
    }

    fn sample_put(&self, _ctx: &CoreContext, _key: &str, _value: &BlobstoreBytes) {}

    fn sample_is_present(&self, _ctx: CoreContext, _key: String, _value: bool) {}
}

// Subcommand entry point for estimate of file compression benefit
pub fn compression_benefit(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<'static, Result<(), Error>> {
    let sizing_sampler = Arc::new(NodeSamplingHandler::<SizingSample>::new());

    match setup_common(
        COMPRESSION_BENEFIT,
        fb,
        &logger,
        Some(sizing_sampler.clone() as Arc<dyn SamplingHandler>),
        matches,
        sub_m,
    ) {
        Err(e) => future::err::<_, Error>(e).boxed(),
        Ok((datasources, walk_params)) => {
            let sizing_state = ProgressStateMutex::new(SizingState::new(logger.clone(), 1));
            let compression_level = args::get_i32_opt(&sub_m, COMPRESSION_LEVEL_ARG).unwrap_or(3);
            let sample_rate = args::get_u64_opt(&sub_m, SAMPLE_RATE_ARG).unwrap_or(100);
            cloned!(
                walk_params.include_node_types,
                walk_params.include_edge_types
            );
            let mut sampling_node_types = match parse_node_types(
                sub_m,
                INCLUDE_SAMPLE_NODE_TYPE_ARG,
                EXCLUDE_SAMPLE_NODE_TYPE_ARG,
                DEFAULT_SAMPLING_NODE_TYPES,
            ) {
                Err(e) => return future::err::<_, Error>(e).boxed(),
                Ok(v) => v,
            };
            sampling_node_types.retain(|i| include_node_types.contains(i));

            let make_sink = {
                cloned!(
                    walk_params.progress_state,
                    walk_params.quiet,
                    walk_params.scheduled_max,
                    sizing_sampler
                );
                move |run: RepoWalkRun| {
                    cloned!(run.ctx);
                    async move |walk_output| {
                        cloned!(ctx, sizing_state);
                        let walk_progress =
                            progress_stream(quiet, &progress_state.clone(), walk_output);

                        let compressor = size_sampling_stream(
                            scheduled_max,
                            walk_progress,
                            CompressorType::Zstd {
                                level: compression_level,
                            },
                            sizing_sampler,
                        );
                        let report_sizing =
                            progress_stream(quiet, &sizing_state.clone(), compressor);
                        report_state(ctx, sizing_state, report_sizing).await
                    }
                }
            };

            let walk_state = WalkState::new(SamplingWalkVisitor::new(
                include_node_types,
                include_edge_types,
                sampling_node_types,
                sizing_sampler,
                sample_rate,
            ));
            walk_exact_tail(fb, logger, datasources, walk_params, walk_state, make_sink).boxed()
        }
    }
}
