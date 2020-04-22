/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{FileContentData, Node, NodeData, NodeType};
use crate::progress::{
    progress_stream, report_state, ProgressReporter, ProgressReporterUnprotected,
    ProgressStateCountByType, ProgressStateMutex,
};
use crate::sampling::{NodeSamplingHandler, PathTrackingRoute, SamplingWalkVisitor};
use crate::setup::{
    parse_node_types, setup_common, COMPRESSION_BENEFIT, COMPRESSION_LEVEL_ARG,
    DEFAULT_INCLUDE_NODE_TYPES, EXCLUDE_SAMPLE_NODE_TYPE_ARG, INCLUDE_SAMPLE_NODE_TYPE_ARG,
    PROGRESS_INTERVAL_ARG, PROGRESS_SAMPLE_DURATION_S, PROGRESS_SAMPLE_RATE,
    PROGRESS_SAMPLE_RATE_ARG, SAMPLE_RATE_ARG,
};
use crate::tail::{walk_exact_tail, RepoWalkRun};

use anyhow::Error;
use async_compression::{metered::MeteredWrite, Compressor, CompressorType};
use bytes::Bytes;
use clap::ArgMatches;
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use derive_more::{Add, Div, Mul, Sub};
use fbinit::FacebookInit;
use futures::{
    future::{self, FutureExt, TryFutureExt},
    stream::{Stream, TryStreamExt},
};
use mononoke_types::BlobstoreBytes;
use samplingblob::SamplingHandler;
use slog::{info, Logger};
use std::{
    cmp::min,
    collections::HashMap,
    fmt,
    io::{Cursor, Write},
    sync::Arc,
    time::Duration,
};

#[derive(Add, Div, Mul, Sub, Clone, Copy, Default, Debug)]
struct SizingStats {
    raw: u64,
    compressed: u64,
}

impl SizingStats {
    fn compression_benefit_pct(&self) -> u64 {
        if self.raw == 0 {
            0
        } else {
            100 * (self.raw - self.compressed) / self.raw
        }
    }
}

impl fmt::Display for SizingStats {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(
            fmt,
            "{},{},{}%",
            self.raw,
            self.compressed,
            self.compression_benefit_pct()
        )
    }
}

fn try_compress(raw_data: &Bytes, compressor_type: CompressorType) -> Result<SizingStats, Error> {
    let raw = raw_data.len() as u64;
    let compressed_buf = MeteredWrite::new(Cursor::new(Vec::with_capacity(4 * 1024)));
    let mut compressor = Compressor::new(compressed_buf, compressor_type);
    compressor.write_all(raw_data)?;
    let compressed_buf = compressor.try_finish().map_err(|(_encoder, e)| e)?;
    // Assume we wouldn't compress if its bigger
    let compressed = min(raw, compressed_buf.total_thru());
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
            // Report the blobstore sizes in sizing stats, more accurate than stream sizes, as headers included
            let sizes = sampler
                .complete_node(&n)
                .map(|sizing_sample| {
                    sizing_sample
                        .data
                        .values()
                        .try_fold(SizingStats::default(), |acc, v| {
                            try_compress(v.as_bytes(), compressor_type).map(|sizes| acc + sizes)
                        })
                })
                .transpose();

            future::ready(sizes.map(|sizes| (n, data_opt, sizes))).right_future()
        }
    })
    .try_buffer_unordered(scheduled_max)
}

impl ProgressStateCountByType<SizingStats, SizingStats> {
    pub fn report_progress_log(self: &mut Self, delta_time: Option<Duration>) {
        let summary_by_type: HashMap<NodeType, SizingStats> = self
            .work_stats
            .stats_by_type
            .iter()
            .map(|(k, (_i, v))| (*k, *v))
            .collect();
        let new_summary = summary_by_type
            .values()
            .fold(SizingStats::default(), |acc, v| acc + *v);
        let delta_summary = new_summary - self.reporting_stats.last_summary;

        let def = SizingStats::default();
        let detail = &self
            .params
            .types_sorted_by_name
            .iter()
            .map(|t| {
                let s = summary_by_type.get(t).unwrap_or(&def);
                format!("{}:{}", t, s)
            })
            .collect::<Vec<_>>()
            .join(" ");

        let (delta_s, delta_summary_per_s) =
            delta_time.map_or((0, SizingStats::default()), |delta_time| {
                (
                    delta_time.as_secs(),
                    delta_summary * 1000 / (delta_time.as_millis() as u64),
                )
            });

        let total_time = self
            .reporting_stats
            .last_update
            .duration_since(self.reporting_stats.start_time);

        let total_summary_per_s = if total_time.as_millis() > 0 {
            new_summary * 1000 / (total_time.as_millis() as u64)
        } else {
            SizingStats::default()
        };

        info!(
            self.params.logger,
            "Raw/s,Compressed/s,Raw,Compressed,%Saving; Delta {:06}/s,{:06}/s,{},{}s; Run {:06}/s,{:06}/s,{},{}s; Type:Raw,Compressed,%Saving {}",
            delta_summary_per_s.raw,
            delta_summary_per_s.compressed,
            delta_summary,
            delta_s,
            total_summary_per_s.raw,
            total_summary_per_s.compressed,
            new_summary,
            total_time.as_secs(),
            detail,
        );

        self.reporting_stats.last_summary_by_type = summary_by_type;
        self.reporting_stats.last_summary = new_summary;
    }
}

impl ProgressReporterUnprotected for ProgressStateCountByType<SizingStats, SizingStats> {
    fn report_progress(self: &mut Self) {
        self.report_progress_log(None);
    }

    fn report_throttled(self: &mut Self) {
        if let Some(delta_time) = self.should_log_throttled() {
            self.report_progress_log(Some(delta_time));
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
}

// Subcommand entry point for estimate of file compression benefit
pub async fn compression_benefit<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a ArgMatches<'a>,
    sub_m: &'a ArgMatches<'a>,
) -> Result<(), Error> {
    let sizing_sampler = Arc::new(NodeSamplingHandler::<SizingSample>::new());

    let (datasources, walk_params) = setup_common(
        COMPRESSION_BENEFIT,
        fb,
        &logger,
        Some(sizing_sampler.clone()),
        matches,
        sub_m,
    )?;

    let repo_stats_key = args::get_repo_name(fb, &matches)?;

    let compression_level = args::get_i32_opt(&sub_m, COMPRESSION_LEVEL_ARG).unwrap_or(3);
    let sample_rate = args::get_u64_opt(&sub_m, SAMPLE_RATE_ARG).unwrap_or(100);
    let progress_interval_secs = args::get_u64_opt(&sub_m, PROGRESS_INTERVAL_ARG);
    let progress_sample_rate = args::get_u64_opt(&sub_m, PROGRESS_SAMPLE_RATE_ARG);

    cloned!(
        walk_params.include_node_types,
        walk_params.include_edge_types
    );
    let mut sampling_node_types = parse_node_types(
        sub_m,
        INCLUDE_SAMPLE_NODE_TYPE_ARG,
        EXCLUDE_SAMPLE_NODE_TYPE_ARG,
        DEFAULT_INCLUDE_NODE_TYPES,
    )?;
    sampling_node_types.retain(|i| include_node_types.contains(i));

    let sizing_progress_state =
        ProgressStateMutex::new(ProgressStateCountByType::<SizingStats, SizingStats>::new(
            fb,
            logger.clone(),
            COMPRESSION_BENEFIT,
            repo_stats_key,
            sampling_node_types.clone(),
            progress_sample_rate.unwrap_or(PROGRESS_SAMPLE_RATE),
            Duration::from_secs(progress_interval_secs.unwrap_or(PROGRESS_SAMPLE_DURATION_S)),
        ));

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
                cloned!(ctx, sizing_progress_state);
                let walk_progress = progress_stream(quiet, &progress_state.clone(), walk_output);

                let compressor = size_sampling_stream(
                    scheduled_max,
                    walk_progress,
                    CompressorType::Zstd {
                        level: compression_level,
                    },
                    sizing_sampler,
                );
                let report_sizing =
                    progress_stream(quiet, &sizing_progress_state.clone(), compressor);
                report_state(ctx, sizing_progress_state, report_sizing)
                    .map({
                        cloned!(progress_state);
                        move |d| {
                            progress_state.report_progress();
                            d
                        }
                    })
                    .await
            }
        }
    };

    let walk_state = Arc::new(SamplingWalkVisitor::new(
        include_node_types,
        include_edge_types,
        sampling_node_types,
        sizing_sampler,
        sample_rate,
    ));
    walk_exact_tail::<_, _, _, _, _, PathTrackingRoute>(
        fb,
        logger,
        datasources,
        walk_params,
        walk_state,
        make_sink,
        true,
    )
    .await
}
