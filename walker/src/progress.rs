/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::graph::{Node, NodeType};
use crate::state::StepStats;
use anyhow::Error;
use cloned::cloned;
use context::CoreContext;
use futures::{
    future::{self},
    Future, Stream,
};
use slog::{info, Logger};
use stats::{define_stats, DynamicTimeseries};
use std::{
    collections::{HashMap, HashSet},
    ops::Add,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

define_stats! {
    prefix = "mononoke.walker";
    walk_progress_walked: dynamic_timeseries("{}.progress.{}.walked", (subcommand: &'static str, repo: String); RATE, SUM),
    walk_progress_queued: dynamic_timeseries("{}.progress.{}.queued", (subcommand: &'static str, repo: String); RATE, SUM),
}

pub trait ProgressRecorderUnprotected<SS> {
    fn record_step(&mut self, n: &Node, ss: Option<&SS>);
}

pub trait ProgressReporterUnprotected {
    /// Report progress unconditional. e.g. called at end of run to report final progress
    fn report_progress(&mut self);

    /// Apply your own throttling criteria, once called might chose to do nothing,
    /// in which case return None, otherwise return how long since last report.
    fn report_throttled(&mut self) -> Option<Duration>;
}

struct ProgressStateByTypeParams {
    logger: Logger,
    subcommand_stats_key: &'static str,
    repo_stats_key: String,
    types_sorted_by_name: Vec<NodeType>,
    throttle_sample_rate: u64,
    throttle_duration: Duration,
}

struct ProgressStateWorkByType<SS>
where
    SS: Add<SS, Output = SS> + Default,
{
    stats_by_type: HashMap<NodeType, (u64, SS)>,
    total_progress: u64,
}

impl<SS> ProgressStateWorkByType<SS>
where
    SS: Add<SS, Output = SS> + Copy + Default,
{
    fn record_step(self: &mut Self, n: &Node, opt: Option<&SS>) {
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
            None => (),
        }
    }
}

struct ProgressStateReporting {
    start_time: Instant,
    last_node_count: u64,
    last_reported: u64,
    last_update: Instant,
}

// Can retain between runs to have cumulative progress reported
pub struct ProgressStateCountByType<SS>
where
    SS: Add<SS, Output = SS> + Default,
{
    params: ProgressStateByTypeParams,
    work_stats: ProgressStateWorkByType<SS>,
    reporting_stats: ProgressStateReporting,
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

impl<SS> ProgressStateCountByType<SS>
where
    SS: Add<SS, Output = SS> + Default,
{
    pub fn new(
        logger: Logger,
        subcommand_stats_key: &'static str,
        repo_stats_key: String,
        included_types: HashSet<NodeType>,
        sample_rate: u64,
        throttle_duration: Duration,
    ) -> Self {
        let types_by_name = sort_by_string(included_types);

        let now = Instant::now();
        Self {
            params: ProgressStateByTypeParams {
                logger,
                subcommand_stats_key,
                repo_stats_key,
                types_sorted_by_name: types_by_name,
                throttle_sample_rate: sample_rate,
                throttle_duration,
            },
            // Updated by record_step
            work_stats: ProgressStateWorkByType::<SS> {
                stats_by_type: HashMap::new(),
                total_progress: 0,
            },
            // Updated by report_*
            reporting_stats: ProgressStateReporting {
                start_time: now,
                last_node_count: 0,
                last_reported: 0,
                last_update: now,
            },
        }
    }
}
impl ProgressStateCountByType<StepStats> {
    fn report_progress_log(self: &mut Self, delta_time: Option<Duration>) {
        let new_node_count = &self
            .work_stats
            .stats_by_type
            .values()
            .map(|(_, ss)| ss.num_expanded_new as u64)
            .sum();
        let delta_progress: u64 =
            self.work_stats.total_progress - self.reporting_stats.last_reported;
        let delta_node_count: u64 = new_node_count - self.reporting_stats.last_node_count;
        let detail = &self
            .params
            .types_sorted_by_name
            .iter()
            .map(|t| {
                let (seen, new_children, visited_of_type) = self
                    .work_stats
                    .stats_by_type
                    .get(t)
                    .map(|(ps, ss)| (*ps, ss.num_expanded_new, ss.visited_of_type))
                    .unwrap_or((0, 0, 0));
                format!("{}:{},{},{}", t, seen, visited_of_type, new_children)
            })
            .collect::<Vec<_>>()
            .join(" ");
        let mut walked_per_s = 0;
        let mut queued_per_s = 0;
        let mut delta_s = 0;
        delta_time.map(|delta_time| {
            delta_s = delta_time.as_secs();
            walked_per_s = delta_progress * 1000 / (delta_time.as_millis() as u64);
            queued_per_s = delta_node_count * 1000 / (delta_time.as_millis() as u64);
        });

        let total_time = self
            .reporting_stats
            .last_update
            .duration_since(self.reporting_stats.start_time);
        let (avg_walked_per_s, avg_queued_per_s) = if total_time.as_millis() > 0 {
            (
                self.work_stats.total_progress * 1000 / (total_time.as_millis() as u64),
                new_node_count * 1000 / (total_time.as_millis() as u64),
            )
        } else {
            (0, 0)
        };

        info!(
            self.params.logger,
            "Walked/s,Children/s,Walked,Children,Time; Delta {:06}/s,{:06}/s,{},{},{}s; Run {:06}/s,{:06}/s,{},{},{}s; Type:Walked,Checks,Children {}",
            walked_per_s,
            queued_per_s,
            delta_progress,
            delta_node_count,
            delta_s,
            avg_walked_per_s,
            avg_queued_per_s,
            self.work_stats.total_progress,
            new_node_count,
            total_time.as_secs(),
            detail,
        );
        self.reporting_stats.last_reported = self.work_stats.total_progress;
        self.reporting_stats.last_node_count = *new_node_count;

        STATS::walk_progress_walked.add_value(
            delta_progress as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
            ),
        );
        STATS::walk_progress_queued.add_value(
            delta_node_count as i64,
            (
                self.params.subcommand_stats_key,
                self.params.repo_stats_key.clone(),
            ),
        );
    }
}

impl<SS> ProgressRecorderUnprotected<SS> for ProgressStateCountByType<SS>
where
    SS: Add<SS, Output = SS> + Copy + Default,
{
    fn record_step(self: &mut Self, n: &Node, opt: Option<&SS>) {
        self.work_stats.record_step(n, opt);
    }
}

impl ProgressReporterUnprotected for ProgressStateCountByType<StepStats> {
    fn report_progress(self: &mut Self) {
        self.report_progress_log(None);
    }

    // Throttle by sample, then time
    fn report_throttled(self: &mut Self) -> Option<Duration> {
        if self.work_stats.total_progress % self.params.throttle_sample_rate == 0 {
            let new_update = Instant::now();
            let delta_time = new_update.duration_since(self.reporting_stats.last_update);
            if delta_time >= self.params.throttle_duration {
                self.report_progress_log(Some(delta_time));
                self.reporting_stats.last_update = new_update;
            }
            Some(delta_time)
        } else {
            None
        }
    }
}

pub trait ProgressRecorder<SS> {
    fn record_step(&self, n: &Node, ss: Option<&SS>);
}

pub trait ProgressReporter {
    fn report_progress(&self);
    fn report_throttled(&self) -> Option<Duration>;
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
}

impl<Inner> ProgressReporter for ProgressStateMutex<Inner>
where
    Inner: ProgressReporterUnprotected,
{
    fn report_progress(&self) {
        self.inner.lock().unwrap().report_progress()
    }

    fn report_throttled(&self) -> Option<Duration> {
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

// Print some status update, passing on all data unchanged
pub fn progress_stream<InStream, PS, ND, SS>(
    quiet: bool,
    progress_state: PS,
    s: InStream,
) -> impl Stream<Item = (Node, Option<ND>, Option<SS>), Error = Error>
where
    InStream: 'static + Stream<Item = (Node, Option<ND>, Option<SS>), Error = Error> + Send,
    PS: 'static + Send + Clone + ProgressRecorder<SS> + ProgressReporter,
{
    s.map(move |(n, data_opt, stats_opt)| {
        progress_state.record_step(&n, stats_opt.as_ref());
        if !quiet {
            progress_state.report_throttled();
        }
        (n, data_opt, stats_opt)
    })
}

// Final status summary, plus count of seen nodes
pub fn report_state<InStream, PS, ND, SS>(
    ctx: CoreContext,
    progress_state: PS,
    s: InStream,
) -> impl Future<Item = (), Error = Error>
where
    InStream: Stream<Item = (Node, Option<ND>, Option<SS>), Error = Error>,
    PS: 'static + Send + Clone + ProgressReporter,
{
    let init_stats: (usize, usize) = (0, 0);
    s.fold(init_stats, {
        move |(mut seen, mut loaded), (_n, nd, _ss)| {
            let data_count = match nd {
                None => 0,
                _ => 1,
            };
            seen += 1;
            loaded += data_count;
            future::ok::<_, Error>((seen, loaded))
        }
    })
    .map({
        cloned!(ctx);
        move |stats| {
            info!(ctx.logger(), "Final count: {:?}", stats);
            progress_state.report_progress();
            ()
        }
    })
}
