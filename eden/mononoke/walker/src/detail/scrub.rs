/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::args::OutputFormat;
use crate::commands::JobParams;
use crate::commands::JobWalkParams;
use crate::commands::RepoSubcommandParams;
use crate::commands::SCRUB;
use crate::detail::graph::FileContentData;
use crate::detail::graph::Node;
use crate::detail::graph::NodeData;
use crate::detail::graph::NodeType;
use crate::detail::graph::WrappedPathHash;
use crate::detail::graph::WrappedPathLike;
use crate::detail::log;
use crate::detail::pack::PackInfo;
use crate::detail::pack::PackInfoLogOptions;
use crate::detail::pack::PackInfoLogger;
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
use crate::detail::sizing::SizingSample;
use crate::detail::tail::walk_exact_tail;
use crate::detail::validate::TOTAL;
use crate::detail::walk::EmptyRoute;
use crate::detail::walk::RepoWalkParams;
use crate::detail::walk::RepoWalkTypeParams;

use anyhow::format_err;
use anyhow::Error;
use blobstore::BlobstoreGetData;
use blobstore::SizeMetadata;
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
use futures::stream::Stream;
use futures::stream::TryStreamExt;
use futures::TryFutureExt;
use metaconfig_types::BlobstoreId;
use mononoke_types::datetime::DateTime;
use samplingblob::ComponentSamplingHandler;
use slog::info;
use stats::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

define_stats! {
    prefix = "mononoke.walker";
    walk_progress_keys: dynamic_timeseries("{}.progress.{}.blobstore_keys", (subcommand: &'static str, repo: String); Rate, Sum),
    walk_progress_bytes: dynamic_timeseries("{}.progress.{}.blobstore_bytes", (subcommand: &'static str, repo: String); Rate, Sum),
    walk_progress_keys_by_type: dynamic_timeseries("{}.progress.{}.{}.blobstore_keys", (subcommand: &'static str, repo: String, node_type: &'static str); Rate, Sum),
    walk_progress_bytes_by_type: dynamic_timeseries("{}.progress.{}.{}.blobstore_bytes", (subcommand: &'static str, repo: String, node_type: &'static str); Rate, Sum),
    walk_last_completed_by_type: dynamic_singleton_counter("{}.last_completed.{}.{}.{}", (subcommand: &'static str, repo: String, node_type: &'static str, desc: &'static str)),
}

#[derive(Add, Div, Mul, Sub, Clone, Copy, Default, Debug)]
pub struct ScrubStats {
    pub blobstore_bytes: u64,
    pub blobstore_keys: u64,
}

impl From<Option<&ScrubSample>> for ScrubStats {
    fn from(sample: Option<&ScrubSample>) -> Self {
        sample
            .map(|sample| ScrubStats {
                blobstore_keys: sample.data.values().len() as u64,
                blobstore_bytes: sample
                    .data
                    .values()
                    // Uncompressed size is always the same between stores, so can take first value
                    .map(|v| v.values().next().map_or(0, |v| v.unique_uncompressed_size))
                    .sum(),
            })
            .unwrap_or_default()
    }
}

impl From<Option<&SizingSample>> for ScrubStats {
    fn from(sample: Option<&SizingSample>) -> Self {
        sample
            .map(|sample| ScrubStats {
                blobstore_keys: sample.data.values().len() as u64,
                blobstore_bytes: sample
                    .data
                    .values()
                    .by_ref()
                    .map(|bytes| bytes.len() as u64)
                    .sum(),
            })
            .unwrap_or_default()
    }
}

impl fmt::Display for ScrubStats {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{},{}", self.blobstore_bytes, self.blobstore_keys,)
    }
}

// Force load of leaf data like file contents that graph traversal did not need
fn loading_stream<InStream, SS, L>(
    limit_data_fetch: bool,
    scheduled_max: usize,
    s: InStream,
    sampler: Arc<WalkSampleMapping<Node, ScrubSample>>,
    output_node_types: HashSet<NodeType>,
    output_format: OutputFormat,
    pack_info_logger: Option<L>,
) -> impl Stream<Item = Result<(Node, Option<NodeData>, Option<ScrubStats>), Error>>
where
    InStream: Stream<
            Item = Result<
                (
                    WalkKeyOptPath<WrappedPathHash>,
                    WalkPayloadMtime,
                    Option<SS>,
                ),
                Error,
            >,
        >
        + 'static
        + Send,
    L: PackInfoLogger + 'static + Send,
{
    s.map_ok(move |(walk_key, payload, _progress_stats)| {
        let mtime = payload.mtime;
        match payload.data {
            Some(NodeData::FileContent(FileContentData::ContentStream(file_bytes_stream)))
                if !limit_data_fetch =>
            {
                cloned!(sampler);
                file_bytes_stream
                    .try_fold(0, |acc, file_bytes| future::ok(acc + file_bytes.size()))
                    .map_ok(move |num_bytes| {
                        let sample = sampler.complete_step(&walk_key.node);
                        (
                            walk_key,
                            mtime,
                            Some(NodeData::FileContent(FileContentData::Consumed(num_bytes))),
                            Some(sample),
                        )
                    })
                    .map_err(|e| e.context(format_err!("While scrubbing file content stream")))
                    .left_future()
            }
            data_opt => {
                if output_node_types.contains(&walk_key.node.get_type()) {
                    match output_format {
                        OutputFormat::Debug => {
                            println!("Node {:?}: NodeData: {:?}", walk_key.node, data_opt)
                        }
                        // Keep Node as non-Pretty so its on same line
                        OutputFormat::PrettyDebug => {
                            println!("Node {:?}: NodeData: {:#?}", walk_key.node, data_opt)
                        }
                    }
                }
                let sample = data_opt
                    .as_ref()
                    .map(|_d| sampler.complete_step(&walk_key.node));
                future::ok((walk_key, mtime, data_opt, sample)).right_future()
            }
        }
    })
    .try_buffer_unordered(scheduled_max)
    .map_ok(move |(walk_key, mtime, data_opt, sample)| {
        let size = if let Some(sample) = sample {
            let size = ScrubStats::from(sample.as_ref());
            if let Some(logger) = pack_info_logger.as_ref() {
                record_for_packer(logger, &walk_key, mtime, sample);
            }
            Some(size)
        } else {
            None
        };
        (walk_key.node, data_opt, size)
    })
}

fn record_for_packer<L>(
    logger: &L,
    walk_key: &WalkKeyOptPath<WrappedPathHash>,
    mtime: Option<DateTime>,
    sample: Option<ScrubSample>,
) where
    L: PackInfoLogger,
{
    if let Some(sample) = sample {
        for (blobstore_key, store_to_key_sizes) in sample.data {
            for (blobstore_id, key_sample) in store_to_key_sizes {
                logger.log(PackInfo {
                    blobstore_id,
                    blobstore_key: blobstore_key.as_str(),
                    node_type: walk_key.node.get_type(),
                    node_fingerprint: walk_key.node.sampling_fingerprint(),
                    similarity_key: walk_key.path.map(|p| p.sampling_fingerprint()),
                    mtime: mtime.map(|mtime| mtime.timestamp_secs() as u64),
                    uncompressed_size: key_sample.unique_uncompressed_size,
                    sizes: key_sample.sizes,
                    ctime: key_sample.ctime,
                })
            }
        }
    }
}

// Sample for one blobstore key
#[derive(Debug)]
struct ScrubKeySample {
    // Every key/store can provide uncompressed size
    unique_uncompressed_size: u64,
    // Only keys accessed via a packblob store have SizeMetadata
    sizes: Option<SizeMetadata>,
    ctime: Option<i64>,
}

// Holds a map from blobstore keys to their samples per store
#[derive(Debug)]
pub struct ScrubSample {
    data: HashMap<String, HashMap<Option<BlobstoreId>, ScrubKeySample>>,
}

impl Default for ScrubSample {
    fn default() -> Self {
        Self {
            data: HashMap::with_capacity(1),
        }
    }
}

impl ComponentSamplingHandler for WalkSampleMapping<Node, ScrubSample> {
    fn sample_get(
        &self,
        ctx: &CoreContext,
        key: &str,
        value: Option<&BlobstoreGetData>,
        inner_id: Option<BlobstoreId>,
    ) -> Result<(), Error> {
        ctx.sampling_key().map(|sampling_key| {
            self.inflight().get_mut(sampling_key).map(|mut guard| {
                value.map(|value| {
                    let sample = ScrubKeySample {
                        unique_uncompressed_size: value.as_bytes().len() as u64,
                        sizes: value.as_meta().sizes().cloned(),
                        ctime: value.as_meta().ctime(),
                    };
                    guard
                        .data
                        .entry(key.to_owned())
                        .or_default()
                        .insert(inner_id, sample)
                })
            })
        });
        Ok(())
    }
}

impl ProgressStateCountByType<ScrubStats, ScrubStats> {
    fn report_stats(&self, node_type: &NodeType, summary: &ScrubStats) {
        STATS::walk_progress_bytes_by_type.add_value(
            summary.blobstore_bytes as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
                node_type.into(),
            ),
        );
        STATS::walk_progress_keys_by_type.add_value(
            summary.blobstore_keys as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
                node_type.into(),
            ),
        );
    }

    fn report_completion_stat(&self, stat: &ScrubStats, stat_key: &'static str) {
        for (desc, value) in &[
            ("blobstore_bytes", stat.blobstore_bytes),
            ("blobstore_keys", stat.blobstore_keys),
        ] {
            STATS::walk_last_completed_by_type.set_value(
                self.params.fb,
                *value as i64,
                (
                    self.params.subcommand_stats_key,
                    self.params.repo_stats_key.clone(),
                    stat_key,
                    desc,
                ),
            );
        }
    }

    fn report_completion_stats(&self) {
        for (k, v) in self.reporting_stats.last_summary_by_type.iter() {
            self.report_completion_stat(v, k.into())
        }
        self.report_completion_stat(&self.reporting_stats.last_summary, TOTAL)
    }

    pub fn report_progress_log(&mut self, delta_time: Option<Duration>) {
        let summary_by_type: HashMap<NodeType, ScrubStats> = self
            .work_stats
            .stats_by_type
            .iter()
            .map(|(k, (_i, v))| (*k, *v))
            .collect();
        for (k, v) in &summary_by_type {
            let delta = *v
                - self
                    .reporting_stats
                    .last_summary_by_type
                    .get(k)
                    .cloned()
                    .unwrap_or_default();
            self.report_stats(k, &delta);
        }
        let new_summary = summary_by_type
            .values()
            .fold(ScrubStats::default(), |acc, v| acc + *v);
        let delta_summary = new_summary - self.reporting_stats.last_summary;

        let def = ScrubStats::default();
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
            delta_time.map_or((0, ScrubStats::default()), |delta_time| {
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
            ScrubStats::default()
        };

        info!(
            self.params.logger,
            #log::SIZING,
            "Bytes/s,Keys/s,Bytes,Keys; Delta {:06}/s,{:06}/s,{},{}s; Run {:06}/s,{:06}/s,{},{}s; Type:Raw,Compressed {}",
            delta_summary_per_s.blobstore_bytes,
            delta_summary_per_s.blobstore_keys,
            delta_summary,
            delta_s,
            total_summary_per_s.blobstore_bytes,
            total_summary_per_s.blobstore_keys,
            new_summary,
            total_time.as_secs(),
            detail,
        );

        STATS::walk_progress_bytes.add_value(
            delta_summary.blobstore_bytes as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
            ),
        );
        STATS::walk_progress_keys.add_value(
            delta_summary.blobstore_keys as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
            ),
        );

        self.reporting_stats.last_summary_by_type = summary_by_type;
        self.reporting_stats.last_summary = new_summary;

        if delta_time.is_none() {
            self.report_completion_stats()
        }
    }
}

impl ProgressReporterUnprotected for ProgressStateCountByType<ScrubStats, ScrubStats> {
    fn report_progress(&mut self) {
        self.report_progress_log(None);
    }

    fn report_throttled(&mut self) {
        if let Some(delta_time) = self.should_log_throttled() {
            self.report_progress_log(Some(delta_time));
        }
    }
}

#[derive(Clone)]
pub struct ScrubCommand {
    pub limit_data_fetch: bool,
    pub output_format: OutputFormat,
    pub output_node_types: HashSet<NodeType>,
    pub progress_options: ProgressOptions,
    pub sampling_options: SamplingOptions,
    pub pack_info_log_options: Option<PackInfoLogOptions>,
    pub sampler: Arc<WalkSampleMapping<Node, ScrubSample>>,
}

impl ScrubCommand {
    fn apply_repo(&mut self, repo_params: &RepoWalkParams) {
        self.sampling_options
            .retain_or_default(&repo_params.include_node_types);
    }
}

// Starts from the graph, (as opposed to walking from blobstore enumeration)
pub async fn scrub_objects(
    fb: FacebookInit,
    job_params: JobParams,
    command: ScrubCommand,
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
    command: ScrubCommand,
    cancellation_requested: Arc<AtomicBool>,
) -> Result<(), Error> {
    let sizing_progress_state =
        ProgressStateMutex::new(ProgressStateCountByType::<ScrubStats, ScrubStats>::new(
            fb,
            repo_params.logger.clone(),
            SCRUB,
            repo_params.repo.name().clone(),
            command.sampling_options.node_types.clone(),
            command.progress_options,
        ));

    let make_sink = {
        cloned!(command, job_params.quiet, sub_params.progress_state,);
        move |ctx: &CoreContext, repo_params: &RepoWalkParams| {
            let repo_name = repo_params.repo.name().clone();
            cloned!(ctx, repo_params.scheduled_max);
            async move |walk_output, run_start, chunk_num, checkpoint_name| {
                let walk_progress = progress_stream(quiet, &progress_state, walk_output);
                let loading = loading_stream(
                    command.limit_data_fetch,
                    scheduled_max,
                    walk_progress,
                    command.sampler,
                    command.output_node_types,
                    command.output_format,
                    command
                        .pack_info_log_options
                        .map(|o| o.make_logger(repo_name, run_start, chunk_num, checkpoint_name)),
                );
                let report_sizing = progress_stream(quiet, &sizing_progress_state, loading);

                report_state(ctx, report_sizing).await?;
                sizing_progress_state.report_progress();
                progress_state.report_progress();
                Ok(())
            }
        }
    };

    let mut stream_node_types = command.output_node_types.clone();
    if !command.limit_data_fetch {
        stream_node_types.insert(NodeType::FileContent);
    }
    if command.pack_info_log_options.is_some() {
        // Need these to be able to see and log the commit time stamps
        stream_node_types.insert(NodeType::Changeset);
        stream_node_types.insert(NodeType::HgChangeset);
    }
    let required_node_data_types: HashSet<NodeType> = stream_node_types.into_iter().collect();

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
        required_node_data_types,
        always_emit_edge_types: HashSet::new(),
        keep_edge_paths: command.pack_info_log_options.is_some(),
    };

    if command.pack_info_log_options.is_some() {
        walk_exact_tail::<_, _, _, _, _, PathTrackingRoute<WrappedPathHash>>(
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
    } else {
        walk_exact_tail::<_, _, _, _, _, EmptyRoute>(
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
}
