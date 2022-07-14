/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::detail::graph::Node;
use crate::detail::graph::NodeType;
use crate::detail::log;
use crate::detail::state::StepStats;
use anyhow::Error;
use context::CoreContext;
use derive_more::Add;
use derive_more::Div;
use derive_more::Mul;
use derive_more::Sub;
use fbinit::FacebookInit;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::info;
use slog::Logger;
use stats::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Add;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

define_stats! {
    prefix = "mononoke.walker";
    walk_progress_walked: dynamic_timeseries("{}.progress.{}.walked", (subcommand: &'static str, repo: String); Rate, Sum),
    walk_progress_queued: dynamic_timeseries("{}.progress.{}.queued", (subcommand: &'static str, repo: String); Rate, Sum),
    walk_progress_errors: dynamic_timeseries("{}.progress.{}.errors", (subcommand: &'static str, repo: String); Rate, Sum),
    walk_progress_missing: dynamic_timeseries("{}.progress.{}.missing", (subcommand: &'static str, repo: String); Rate, Sum),
    walk_progress_hash_validation_failure: dynamic_timeseries("{}.progress.{}.hash_validation_failure", (subcommand: &'static str, repo: String); Rate, Sum),
    walk_progress_walked_by_type: dynamic_timeseries("{}.progress.{}.{}.walked", (subcommand: &'static str, repo: String, node_type: String); Rate, Sum),
    walk_progress_errors_by_type: dynamic_timeseries("{}.progress.{}.{}.errors", (subcommand: &'static str, repo: String, node_type: String); Rate, Sum),
    walk_progress_missing_by_type: dynamic_timeseries("{}.progress.{}.{}.missing", (subcommand: &'static str, repo: String, node_type: String); Rate, Sum),
    walk_progress_hash_validation_failure_by_type: dynamic_timeseries("{}.progress.{}.{}.hash_validation_failure", (subcommand: &'static str, repo: String, node_type: String); Rate, Sum),
}

pub trait ProgressRecorderUnprotected<SS> {
    fn record_step(&mut self, n: &Node, ss: Option<&SS>);

    fn set_sample_builder(&mut self, s: MononokeScubaSampleBuilder);
}

pub trait ProgressReporterUnprotected {
    /// Report progress unconditional. e.g. called at end of run to report final progress
    fn report_progress(&mut self);

    /// Apply your own throttling criteria, once called might chose to do nothing,
    /// in which case return None, otherwise return how long since last report.
    fn report_throttled(&mut self);
}

#[derive(Clone, Copy)]
pub struct ProgressOptions {
    pub sample_rate: u64,
    pub interval: Duration,
}

pub struct ProgressStateByTypeParams {
    pub fb: FacebookInit,
    pub logger: Logger,
    pub subcommand_stats_key: &'static str,
    pub repo_stats_key: String,
    pub types_sorted_by_name: Vec<NodeType>,
    options: ProgressOptions,
}

pub struct ProgressStateWorkByType<SS>
where
    SS: Add<SS, Output = SS> + Default,
{
    pub stats_by_type: HashMap<NodeType, (u64, SS)>,
    total_progress: u64,
}

impl<SS> ProgressStateWorkByType<SS>
where
    SS: Add<SS, Output = SS> + Copy + Default,
{
    fn record_step(&mut self, n: &Node, opt: Option<&SS>) {
        // Global stats
        self.total_progress += 1;
        // By type
        let k = n.get_type();
        let mut entry = self.stats_by_type.entry(k).or_insert((0, SS::default()));
        entry.0 += 1;
        match opt {
            Some(ss) => {
                entry.1 = entry.1 + *ss;
            }
            None => {}
        }
    }
}

#[derive(Add, Sub, Mul, Div, Clone, Copy, Default, Debug)]
pub struct ProgressSummary {
    walked: u64,
    checked: u64,
    queued: u64,
    errors: u64,
    missing: u64,
    hash_validation_failure: u64,
}

// Takes a summary type as a parameter. e.g. ProgressSummary
pub struct ProgressStateReporting<T> {
    pub start_time: Instant,
    pub last_summary_by_type: HashMap<NodeType, T>,
    pub last_summary: T,
    pub last_update: Instant,
}

// Can retain between runs to have cumulative progress reported
pub struct ProgressStateCountByType<SS, T>
where
    SS: Add<SS, Output = SS> + Default,
{
    pub params: ProgressStateByTypeParams,
    pub work_stats: ProgressStateWorkByType<SS>,
    pub reporting_stats: ProgressStateReporting<T>,
}

pub fn sort_by_string<C, T>(c: C) -> Vec<T>
where
    C: IntoIterator<Item = T>,
    T: ToString,
{
    let mut v: Vec<_> = c.into_iter().collect();
    v.sort_by_key(|k| k.to_string());
    v
}

impl<SS, T> ProgressStateCountByType<SS, T>
where
    SS: Add<SS, Output = SS> + Default,
    T: Default,
{
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        subcommand_stats_key: &'static str,
        repo_stats_key: String,
        included_types: HashSet<NodeType>,
        options: ProgressOptions,
    ) -> Self {
        let types_by_name = sort_by_string(included_types);

        let now = Instant::now();
        Self {
            params: ProgressStateByTypeParams {
                fb,
                logger,
                subcommand_stats_key,
                repo_stats_key,
                types_sorted_by_name: types_by_name,
                options,
            },
            // Updated by record_step
            work_stats: ProgressStateWorkByType::<SS> {
                stats_by_type: HashMap::new(),
                total_progress: 0,
            },
            // Updated by report_*
            reporting_stats: ProgressStateReporting::<T> {
                start_time: now,
                last_summary_by_type: HashMap::new(),
                last_summary: T::default(),
                last_update: now,
            },
        }
    }

    // Throttle by sample, then time
    pub fn should_log_throttled(&mut self) -> Option<Duration> {
        if self.work_stats.total_progress % self.params.options.sample_rate == 0 {
            let new_update = Instant::now();
            let delta_time = new_update.duration_since(self.reporting_stats.last_update);
            if delta_time >= self.params.options.interval {
                self.reporting_stats.last_update = new_update;
                return Some(delta_time);
            }
        }
        None
    }
}

impl ProgressStateCountByType<StepStats, ProgressSummary> {
    fn report_stats(&self, node_type: &NodeType, summary: &ProgressSummary) {
        STATS::walk_progress_walked_by_type.add_value(
            summary.walked as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
                node_type.to_string(),
            ),
        );
        STATS::walk_progress_errors_by_type.add_value(
            summary.errors as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
                node_type.to_string(),
            ),
        );
        STATS::walk_progress_missing_by_type.add_value(
            summary.missing as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
                node_type.to_string(),
            ),
        );
        STATS::walk_progress_hash_validation_failure_by_type.add_value(
            summary.hash_validation_failure as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
                node_type.to_string(),
            ),
        );
    }

    pub fn report_progress_log(&mut self, mut delta_time: Option<Duration>) {
        let summary_by_type: HashMap<NodeType, ProgressSummary> = self
            .work_stats
            .stats_by_type
            .iter()
            .map(|(k, (ps, ss))| {
                let s = ProgressSummary {
                    walked: *ps,
                    checked: ss.visited_of_type as u64,
                    // num_expanded_new is per type children which when summed == a top level queued stat
                    queued: ss.num_expanded_new as u64,
                    errors: ss.error_count as u64,
                    missing: ss.missing_count as u64,
                    hash_validation_failure: ss.hash_validation_failure_count as u64,
                };
                let delta = s - self
                    .reporting_stats
                    .last_summary_by_type
                    .get(k)
                    .cloned()
                    .unwrap_or_default();
                self.report_stats(k, &delta);
                (*k, s)
            })
            .collect();

        let new_summary = summary_by_type
            .values()
            .fold(ProgressSummary::default(), |acc, v| acc + *v);
        let delta_summary = new_summary - self.reporting_stats.last_summary;

        let detail = &self
            .params
            .types_sorted_by_name
            .iter()
            .map(|t| {
                let s = summary_by_type.get(t).cloned().unwrap_or_default();
                format!("{}:{},{},{}", t, s.walked, s.checked, s.queued)
            })
            .collect::<Vec<_>>()
            .join(" ");

        if delta_time.is_none() {
            // Is the last log of a run or chunk, need to know the time
            let now = Instant::now();
            let t = now.duration_since(self.reporting_stats.last_update);
            delta_time = if t.as_millis() > 0 { Some(t) } else { None };
            self.reporting_stats.last_update = now;
        }

        let (delta_s, delta_summary_per_s) =
            delta_time.map_or((0, ProgressSummary::default()), |delta_time| {
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
            ProgressSummary::default()
        };

        info!(
            self.params.logger,
            #log::GRAPH,
            "Walked/s,Children/s,Walked,Errors,Missing,Children,Time; Delta {:06}/s,{:06}/s,{},{},{},{},{}s; Run {:06}/s,{:06}/s,{},{},{},{},{}s; Type:Walked,Checks,Children {}",
            delta_summary_per_s.walked,
            delta_summary_per_s.queued,
            delta_summary.walked,
            delta_summary.errors,
            delta_summary.missing,
            delta_summary.queued,
            delta_s,
            total_summary_per_s.walked,
            total_summary_per_s.queued,
            self.work_stats.total_progress,
            new_summary.errors,
            new_summary.missing,
            new_summary.queued,
            total_time.as_secs(),
            detail,
        );

        STATS::walk_progress_walked.add_value(
            delta_summary.walked as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
            ),
        );
        STATS::walk_progress_queued.add_value(
            delta_summary.queued as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
            ),
        );
        STATS::walk_progress_errors.add_value(
            delta_summary.errors as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
            ),
        );

        STATS::walk_progress_missing.add_value(
            delta_summary.missing as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
            ),
        );

        STATS::walk_progress_hash_validation_failure.add_value(
            delta_summary.hash_validation_failure as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
            ),
        );

        self.reporting_stats.last_summary_by_type = summary_by_type;
        self.reporting_stats.last_summary = new_summary;
    }
}

impl<SS, T> ProgressRecorderUnprotected<SS> for ProgressStateCountByType<SS, T>
where
    SS: Add<SS, Output = SS> + Copy + Default,
{
    fn record_step(&mut self, n: &Node, opt: Option<&SS>) {
        self.work_stats.record_step(n, opt);
    }

    fn set_sample_builder(&mut self, _s: MononokeScubaSampleBuilder) {
        // NOOP
    }
}

impl ProgressReporterUnprotected for ProgressStateCountByType<StepStats, ProgressSummary> {
    fn report_progress(&mut self) {
        self.report_progress_log(None);
    }

    fn report_throttled(&mut self) {
        if let Some(delta_time) = self.should_log_throttled() {
            self.report_progress_log(Some(delta_time));
        }
    }
}

pub trait ProgressRecorder<SS> {
    fn record_step(&self, n: &Node, ss: Option<&SS>);
    fn set_sample_builder(&self, s: MononokeScubaSampleBuilder);
}

pub trait ProgressReporter {
    fn report_progress(&self);
    fn report_throttled(&self);
}

#[derive(Debug)]
pub struct ProgressStateMutex<Inner> {
    inner: Arc<Mutex<Inner>>,
}

impl<Inner> ProgressStateMutex<Inner> {
    pub fn new(inner: Inner) -> Self {
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }
}

impl<Inner, SS> ProgressRecorder<SS> for ProgressStateMutex<Inner>
where
    Inner: ProgressRecorderUnprotected<SS>,
{
    fn record_step(&self, n: &Node, ss: Option<&SS>) {
        self.inner.lock().unwrap().record_step(n, ss)
    }

    fn set_sample_builder(&self, s: MononokeScubaSampleBuilder) {
        self.inner.lock().unwrap().set_sample_builder(s)
    }
}

impl<Inner> ProgressReporter for ProgressStateMutex<Inner>
where
    Inner: ProgressReporterUnprotected,
{
    fn report_progress(&self) {
        self.inner.lock().unwrap().report_progress()
    }

    fn report_throttled(&self) {
        self.inner.lock().unwrap().report_throttled()
    }
}

impl<Inner> Clone for ProgressStateMutex<Inner> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

// Log some status update, passing on all data unchanged
pub fn progress_stream<InStream, PS, Payload, SS, K>(
    quiet: bool,
    progress_state: &PS,
    s: InStream,
) -> impl Stream<Item = Result<(K, Payload, Option<SS>), Error>>
where
    InStream: Stream<Item = Result<(K, Payload, Option<SS>), Error>> + 'static + Send,
    PS: 'static + Send + Clone + ProgressRecorder<SS> + ProgressReporter,
    K: 'static,
    // Make sure we can convert from K reference to Node reference
    for<'b> &'b Node: From<&'b K>,
{
    s.map({
        let progress_state = progress_state.clone();
        move |r| {
            r.map(|(key, payload, stats_opt)| {
                {
                    let k: &K = &key;
                    let n: &Node = k.into();
                    progress_state.record_step(n, stats_opt.as_ref());
                    if !quiet {
                        progress_state.report_throttled();
                    }
                }
                (key, payload, stats_opt)
            })
        }
    })
}

// Final status summary, plus count of seen nodes
pub async fn report_state<InStream, ND, SS>(ctx: CoreContext, s: InStream) -> Result<(), Error>
where
    InStream: Stream<Item = Result<(Node, Option<ND>, Option<SS>), Error>> + 'static + Send,
{
    let (seen, loaded) = s
        .try_fold((0_usize, 0_usize), {
            async move |(mut seen, mut loaded), (_n, nd, _ss)| {
                seen += 1;
                if nd.is_some() {
                    loaded += 1;
                }
                Ok((seen, loaded))
            }
        })
        .await?;

    info!(ctx.logger(), #log::LOADED, "Seen,Loaded: {},{}", seen, loaded);
    Ok(())
}
