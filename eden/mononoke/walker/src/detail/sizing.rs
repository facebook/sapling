/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::commands::JobParams;
use crate::commands::JobWalkParams;
use crate::commands::RepoSubcommandParams;
use crate::commands::COMPRESSION_BENEFIT;
use crate::detail::graph::FileContentData;
use crate::detail::graph::Node;
use crate::detail::graph::NodeData;
use crate::detail::graph::NodeType;
use crate::detail::graph::WrappedPath;
use crate::detail::progress::progress_stream;
use crate::detail::progress::report_state;
use crate::detail::progress::ProgressOptions;
use crate::detail::progress::ProgressReporter;
use crate::detail::progress::ProgressReporterUnprotected;
use crate::detail::progress::ProgressStateCountByType;
use crate::detail::progress::ProgressStateMutex;
use crate::detail::sampling::PathTrackingRoute;
use crate::detail::sampling::SamplingOptions;
use crate::detail::sampling::SamplingWalkVisitor;
use crate::detail::sampling::WalkKeyOptPath;
use crate::detail::sampling::WalkPayloadMtime;
use crate::detail::sampling::WalkSampleMapping;
use crate::detail::tail::walk_exact_tail;
use crate::detail::walk::RepoWalkParams;
use crate::detail::walk::RepoWalkTypeParams;

use anyhow::Error;
use async_compression::metered::MeteredWrite;
use async_compression::Compressor;
use async_compression::CompressorType;
use blobstore::BlobstoreGetData;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use derive_more::Add;
use derive_more::Div;
use derive_more::Mul;
use derive_more::Sub;
use fbinit::FacebookInit;
use futures::future;
use futures::future::try_join_all;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream::Stream;
use futures::stream::TryStreamExt;
use maplit::hashset;
use mononoke_types::BlobstoreBytes;
use samplingblob::SamplingHandler;
use slog::info;
use std::cmp::min;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::io::Cursor;
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

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
    sampler: Arc<WalkSampleMapping<Node, SizingSample>>,
) -> impl Stream<Item = Result<(Node, Option<NodeData>, Option<SizingStats>), Error>>
where
    InStream: Stream<
            Item = Result<
                (
                    WalkKeyOptPath<WrappedPath>,
                    Option<NodeData>,
                    Option<InStats>,
                ),
                Error,
            >,
        >
        + 'static
        + Send,
    InStats: 'static + Send,
{
    s.map_ok(move |(walk_key, data_opt, _stats_opt)| {
        match (&walk_key.node, data_opt) {
            (Node::FileContent(_content_id), Some(NodeData::FileContent(fc)))
                if sampler.is_sampling(&walk_key.node) =>
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
                            .complete_step(&walk_key.node)
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
                                walk_key.node,
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
                    .complete_step(&walk_key.node)
                    .map(|sizing_sample| {
                        sizing_sample
                            .data
                            .values()
                            .try_fold(SizingStats::default(), |acc, v| {
                                try_compress(v.as_bytes(), compressor_type).map(|sizes| acc + sizes)
                            })
                    })
                    .transpose();

                future::ready(sizes.map(|sizes| (walk_key.node, data_opt, sizes))).right_future()
            }
        }
    })
    .try_buffer_unordered(scheduled_max)
}

impl ProgressStateCountByType<SizingStats, SizingStats> {
    pub fn report_progress_log(&mut self, delta_time: Option<Duration>) {
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
    fn report_progress(&mut self) {
        self.report_progress_log(None);
    }

    fn report_throttled(&mut self) {
        if let Some(delta_time) = self.should_log_throttled() {
            self.report_progress_log(Some(delta_time));
        }
    }
}

#[derive(Debug)]
pub struct SizingSample {
    pub data: HashMap<String, BlobstoreBytes>,
}

impl Default for SizingSample {
    fn default() -> Self {
        Self {
            data: HashMap::with_capacity(1),
        }
    }
}

impl SamplingHandler for WalkSampleMapping<Node, SizingSample> {
    fn sample_get(
        &self,
        ctx: &CoreContext,
        key: &str,
        value: Option<&BlobstoreGetData>,
    ) -> Result<(), Error> {
        ctx.sampling_key().map(|sampling_key| {
            self.inflight().get_mut(sampling_key).map(|mut guard| {
                value.map(|value| guard.data.insert(key.to_owned(), value.as_bytes().clone()))
            })
        });
        Ok(())
    }
}

#[derive(Clone)]
pub struct SizingCommand {
    pub compression_level: i32,
    pub progress_options: ProgressOptions,
    pub sampling_options: SamplingOptions,
    pub sampler: Arc<WalkSampleMapping<Node, SizingSample>>,
}

impl SizingCommand {
    fn apply_repo(&mut self, repo_params: &RepoWalkParams) {
        self.sampling_options
            .retain_or_default(&repo_params.include_node_types);
    }
}

// Subcommand entry point for estimate of file compression benefit
pub async fn compression_benefit(
    fb: FacebookInit,
    job_params: JobParams,
    command: SizingCommand,
    cancellation_requested: Arc<AtomicBool>,
) -> Result<(), Error> {
    let JobParams {
        walk_params,
        per_repo,
    } = job_params;

    let mut all_walks = Vec::new();
    for (sub_params, repo_params) in per_repo {
        cloned!(mut command, walk_params);

        command.apply_repo(&repo_params);

        let walk = run_one(
            fb,
            walk_params,
            sub_params,
            repo_params,
            command,
            Arc::clone(&cancellation_requested),
        );
        all_walks.push(walk);
    }
    try_join_all(all_walks).await.map(|_| ())
}

async fn run_one(
    fb: FacebookInit,
    job_params: JobWalkParams,
    sub_params: RepoSubcommandParams,
    repo_params: RepoWalkParams,
    command: SizingCommand,
    cancellation_requested: Arc<AtomicBool>,
) -> Result<(), Error> {
    let sizing_progress_state =
        ProgressStateMutex::new(ProgressStateCountByType::<SizingStats, SizingStats>::new(
            fb,
            repo_params.logger.clone(),
            COMPRESSION_BENEFIT,
            repo_params.repo.name().clone(),
            command.sampling_options.node_types.clone(),
            command.progress_options,
        ));

    let make_sink = {
        cloned!(command, job_params.quiet, sub_params.progress_state,);
        move |ctx: &CoreContext, repo_params: &RepoWalkParams| {
            cloned!(ctx, repo_params.scheduled_max);
            async move |walk_output, _run_start, _chunk_num, _checkpoint_name| {
                cloned!(ctx, sizing_progress_state);
                // Sizing doesn't use mtime, so remove it from payload
                let walk_progress = progress_stream(quiet, &progress_state, walk_output).map_ok(
                    |(key, payload, stats): (_, WalkPayloadMtime, _)| (key, payload.data, stats),
                );

                let compressor = size_sampling_stream(
                    scheduled_max,
                    walk_progress,
                    CompressorType::Zstd {
                        level: command.compression_level,
                    },
                    command.sampler,
                );
                let report_sizing = progress_stream(quiet, &sizing_progress_state, compressor);

                report_state(ctx, report_sizing).await?;
                sizing_progress_state.report_progress();
                progress_state.report_progress();
                Ok(())
            }
        }
    };

    let walk_state = SamplingWalkVisitor::new(
        repo_params.include_node_types.clone(),
        repo_params.include_edge_types.clone(),
        command.sampling_options,
        None,
        command.sampler,
        job_params.enable_derive,
        sub_params
            .tail_params
            .chunking
            .as_ref()
            .map(|v| v.direction),
    );

    let type_params = RepoWalkTypeParams {
        required_node_data_types: hashset![NodeType::FileContent],
        always_emit_edge_types: HashSet::new(),
        keep_edge_paths: true,
    };

    walk_exact_tail::<_, _, _, _, _, PathTrackingRoute<WrappedPath>>(
        fb,
        job_params,
        repo_params,
        type_params,
        sub_params.tail_params,
        walk_state,
        make_sink,
        cancellation_requested,
    )
    .await
}
