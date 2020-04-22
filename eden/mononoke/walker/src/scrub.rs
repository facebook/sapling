/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::args;
use crate::graph::{FileContentData, Node, NodeData, NodeType};
use crate::progress::{
    progress_stream, report_state, ProgressReporter, ProgressReporterUnprotected,
    ProgressStateCountByType, ProgressStateMutex,
};
use crate::sampling::{NodeSamplingHandler, SamplingWalkVisitor};
use crate::setup::{
    parse_node_types, setup_common, DEFAULT_INCLUDE_NODE_TYPES, EXCLUDE_SAMPLE_NODE_TYPE_ARG,
    INCLUDE_SAMPLE_NODE_TYPE_ARG, LIMIT_DATA_FETCH_ARG, PROGRESS_SAMPLE_DURATION_S,
    SAMPLE_RATE_ARG, SCRUB,
};
use crate::state::WalkState;
use crate::tail::{walk_exact_tail, RepoWalkRun};
use crate::validate::TOTAL;

use anyhow::Error;
use clap::ArgMatches;
use cloned::cloned;
use context::CoreContext;
use derive_more::{Add, Div, Mul, Sub};
use fbinit::FacebookInit;
use futures::{
    future::{self, BoxFuture, FutureExt},
    stream::{Stream, TryStreamExt},
    TryFutureExt,
};
use mononoke_types::BlobstoreBytes;
use samplingblob::SamplingHandler;
use slog::{info, Logger};
use stats::prelude::*;
use std::{collections::HashMap, fmt, sync::Arc, time::Duration};

define_stats! {
    prefix = "mononoke.walker";
    walk_progress_keys: dynamic_timeseries("{}.progress.{}.blobstore_keys", (subcommand: &'static str, repo: String); Rate, Sum),
    walk_progress_bytes: dynamic_timeseries("{}.progress.{}.blobstore_bytes", (subcommand: &'static str, repo: String); Rate, Sum),
    walk_progress_keys_by_type: dynamic_timeseries("{}.progress.{}.{}.blobstore_keys", (subcommand: &'static str, repo: String, node_type: &'static str); Rate, Sum),
    walk_progress_bytes_by_type: dynamic_timeseries("{}.progress.{}.{}.blobstore_bytes", (subcommand: &'static str, repo: String, node_type: &'static str); Rate, Sum),
    walk_last_completed_by_type: dynamic_singleton_counter("{}.last_completed.{}.{}.{}", (subcommand: &'static str, repo: String, node_type: &'static str, desc: &'static str)),
}

#[derive(Add, Div, Mul, Sub, Clone, Copy, Default, Debug)]
struct ScrubStats {
    blobstore_bytes: u64,
    blobstore_keys: u64,
}

impl ScrubStats {
    fn new(sample: Option<ScrubSample>) -> Self {
        sample
            .map(|sample| ScrubStats {
                blobstore_keys: sample.data.values().len() as u64,
                blobstore_bytes: sample.data.values().sum(),
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
fn loading_stream<InStream, SS>(
    limit_data_fetch: bool,
    scheduled_max: usize,
    s: InStream,
    sampler: Arc<NodeSamplingHandler<ScrubSample>>,
) -> impl Stream<Item = Result<(Node, Option<NodeData>, Option<ScrubStats>), Error>>
where
    InStream: Stream<Item = Result<(Node, Option<NodeData>, Option<SS>), Error>> + 'static + Send,
{
    s.map_ok(move |(n, nd, _progress_stats)| match nd {
        Some(NodeData::FileContent(FileContentData::ContentStream(file_bytes_stream)))
            if !limit_data_fetch =>
        {
            cloned!(sampler);
            file_bytes_stream
                .try_fold(0, |acc, file_bytes| future::ok(acc + file_bytes.size()))
                .map_ok(move |num_bytes| {
                    let size = ScrubStats::new(sampler.complete_node(&n));
                    (
                        n,
                        Some(NodeData::FileContent(FileContentData::Consumed(num_bytes))),
                        Some(size),
                    )
                })
                .left_future()
        }
        data_opt => {
            let size = data_opt
                .as_ref()
                .map(|_d| ScrubStats::new(sampler.complete_node(&n)));
            future::ok((n, data_opt, size)).right_future()
        }
    })
    .try_buffer_unordered(scheduled_max)
}

#[derive(Debug)]
struct ScrubSample {
    data: HashMap<String, u64>,
}

impl Default for ScrubSample {
    fn default() -> Self {
        Self {
            data: HashMap::with_capacity(1),
        }
    }
}

impl SamplingHandler for NodeSamplingHandler<ScrubSample> {
    fn sample_get(&self, ctx: CoreContext, key: String, value: Option<&BlobstoreBytes>) {
        ctx.sampling_key().map(|sampling_key| {
            self.inflight().get_mut(sampling_key).map(|mut guard| {
                value.map(|value| guard.data.insert(key.clone(), value.len() as u64))
            })
        });
    }
    fn sample_put(&self, _ctx: &CoreContext, _key: &str, _value: &BlobstoreBytes) {}
    fn sample_is_present(&self, _ctx: CoreContext, _key: String, _value: bool) {}
}

impl ProgressStateCountByType<ScrubStats, ScrubStats> {
    fn report_stats(&self, node_type: &NodeType, summary: &ScrubStats) {
        STATS::walk_progress_bytes_by_type.add_value(
            summary.blobstore_bytes as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
                node_type.to_str(),
            ),
        );
        STATS::walk_progress_keys_by_type.add_value(
            summary.blobstore_keys as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
                node_type.to_str(),
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
            self.report_completion_stat(v, k.to_str())
        }
        self.report_completion_stat(&self.reporting_stats.last_summary, TOTAL)
    }

    pub fn report_progress_log(self: &mut Self, delta_time: Option<Duration>) {
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
    fn report_progress(self: &mut Self) {
        self.report_progress_log(None);
    }

    fn report_throttled(self: &mut Self) {
        if let Some(delta_time) = self.should_log_throttled() {
            self.report_progress_log(Some(delta_time));
        }
    }
}

// Starts from the graph, (as opposed to walking from blobstore enumeration)
pub fn scrub_objects(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<'static, Result<(), Error>> {
    let scrub_sampler = Arc::new(NodeSamplingHandler::<ScrubSample>::new());

    match setup_common(
        SCRUB,
        fb,
        &logger,
        Some(scrub_sampler.clone()),
        matches,
        sub_m,
    )
    .and_then(|(datasources, walk_params)| {
        args::get_repo_name(fb, &matches)
            .map(|repo_stats_key| (datasources, walk_params, repo_stats_key))
    }) {
        Err(e) => future::err::<_, Error>(e).boxed(),
        Ok((datasources, walk_params, repo_stats_key)) => {
            let sample_rate = args::get_u64_opt(&sub_m, SAMPLE_RATE_ARG).unwrap_or(1);
            let limit_data_fetch = sub_m.is_present(LIMIT_DATA_FETCH_ARG);
            let scheduled_max = walk_params.scheduled_max;
            let quiet = walk_params.quiet;
            let progress_state = walk_params.progress_state.clone();

            cloned!(
                walk_params.include_node_types,
                walk_params.include_edge_types
            );
            let mut sampling_node_types = match parse_node_types(
                sub_m,
                INCLUDE_SAMPLE_NODE_TYPE_ARG,
                EXCLUDE_SAMPLE_NODE_TYPE_ARG,
                DEFAULT_INCLUDE_NODE_TYPES,
            ) {
                Err(e) => return future::err::<_, Error>(e).boxed(),
                Ok(v) => v,
            };
            sampling_node_types.retain(|i| include_node_types.contains(i));

            let sizing_progress_state =
                ProgressStateMutex::new(ProgressStateCountByType::<ScrubStats, ScrubStats>::new(
                    fb,
                    logger.clone(),
                    SCRUB,
                    repo_stats_key,
                    sampling_node_types.clone(),
                    sample_rate,
                    Duration::from_secs(PROGRESS_SAMPLE_DURATION_S),
                ));

            let make_sink = {
                cloned!(scrub_sampler);
                move |run: RepoWalkRun| {
                    cloned!(run.ctx);
                    async move |walk_output| {
                        let walk_progress = progress_stream(quiet, &progress_state, walk_output);
                        let loading = loading_stream(
                            limit_data_fetch,
                            scheduled_max,
                            walk_progress,
                            scrub_sampler,
                        );
                        let report_sizing =
                            progress_stream(quiet, &sizing_progress_state.clone(), loading);

                        report_state(ctx, sizing_progress_state, report_sizing)
                            .map_ok({
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

            let walk_state = WalkState::new(SamplingWalkVisitor::new(
                include_node_types,
                include_edge_types,
                sampling_node_types,
                scrub_sampler,
                sample_rate,
            ));
            walk_exact_tail::<_, _, _, _, _, ()>(
                fb,
                logger,
                datasources,
                walk_params,
                walk_state,
                make_sink,
            )
            .boxed()
        }
    }
}
