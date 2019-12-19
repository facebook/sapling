/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

// This module allows the implementation of validating checks over the mononoke graph
// Currently checks are added by
//  1. Add a CheckType variant
//  2. Add CheckType::node_type() and CheckType::enum_type() cases for the new variant
//  3. Add a new validation method
//  4. Add the method to the match/case in ValidatingVisitor::visit()

use crate::graph::{EdgeType, Node, NodeType};
use crate::progress::{
    progress_stream, report_state, sort_by_string, ProgressRecorderUnprotected, ProgressReporter,
    ProgressReporterUnprotected, ProgressStateCountByType, ProgressStateMutex,
};
use crate::setup::{setup_common, EXCLUDE_CHECK_TYPE_ARG, INCLUDE_CHECK_TYPE_ARG};
use crate::state::{StepStats, WalkState, WalkStateCHashMap};
use crate::tail::walk_exact_tail;
use crate::walk::{OutgoingEdge, ResolvedNode, WalkVisitor};

use anyhow::{format_err, Error};
use clap::ArgMatches;
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{Future, Stream};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use itertools::Itertools;
use slog::{info, warn, Logger};
use stats::service_data::{get_service_data_singleton, ServiceData};
use stats::{define_stats, DynamicTimeseries};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    hash::Hash,
    iter::FromIterator,
    ops::AddAssign,
    str::FromStr,
    time::{Duration, Instant},
};

pub const STATS_PREFIX: &'static str = "mononoke.walker.validate";
pub const NODES: &'static str = "nodes";
pub const EDGES: &'static str = "edges";
pub const PASS: &'static str = "pass";
pub const FAIL: &'static str = "fail";
pub const TOTAL: &'static str = "total";

define_stats! {
    prefix = "mononoke.walker.validate";
    // e.g. mononoke.walker.validate.testrepo.hg_link_node_populated.pass
    walker_validate: dynamic_timeseries("{}.{}.{}", (repo: String, check: &'static str, status: &'static str); RATE, SUM),
}

pub const DEFAULT_CHECK_TYPES: &[CheckType] = &[CheckType::HgLinkNodePopulated];

const PROGRESS_SAMPLE_RATE: u64 = 1000;
const PROGRESS_SAMPLE_DURATION_S: u64 = 5;
const VALIDATE_PROGRESS_SAMPLE_DURATION_S: u64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum CheckStatus {
    // could add more info as parameter to Fail if needed
    Fail,
    Pass,
}

define_type_enum! {
enum CheckType {
    HgLinkNodePopulated,
}
}

impl CheckType {
    fn stats_key(&self) -> &'static str {
        match self {
            CheckType::HgLinkNodePopulated => "hg_link_node_populated",
        }
    }
    pub fn node_type(&self) -> NodeType {
        match self {
            CheckType::HgLinkNodePopulated => NodeType::HgFileNode,
        }
    }
}

impl fmt::Display for CheckType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug)]
struct CheckOutput {
    check: CheckType,
    status: CheckStatus,
}

impl CheckOutput {
    fn new(check: CheckType, status: CheckStatus) -> Self {
        Self { check, status }
    }
}

struct ValidatingVisitor {
    repo_stats_key: String,
    inner: WalkStateCHashMap,
    checks_by_node_type: HashMap<NodeType, HashSet<CheckType>>,
}

impl ValidatingVisitor {
    pub fn new(
        repo_stats_key: String,
        include_node_types: HashSet<NodeType>,
        include_edge_types: HashSet<EdgeType>,
        include_checks: HashSet<CheckType>,
    ) -> Self {
        Self {
            repo_stats_key,
            inner: WalkStateCHashMap::new(include_node_types, include_edge_types),
            checks_by_node_type: include_checks
                .into_iter()
                .group_by(|c| c.node_type())
                .into_iter()
                .map(|(key, group)| (key, HashSet::from_iter(group)))
                .collect(),
        }
    }
}

fn check_linknode_populated(outgoing: &[OutgoingEdge]) -> CheckStatus {
    if outgoing
        .iter()
        .any(|e| e.label == EdgeType::HgLinkNodeToHgChangeset)
    {
        CheckStatus::Pass
    } else {
        CheckStatus::Fail
    }
}

#[derive(Clone, Copy, Default, Debug)]
struct CheckStats {
    pass: u64,
    fail: u64,
    edges: u64,
}

impl AddAssign for CheckStats {
    fn add_assign(&mut self, other: Self) {
        *self = Self {
            pass: self.pass + other.pass,
            fail: self.fail + other.fail,
            edges: self.edges + other.edges,
        };
    }
}

struct CheckData {
    checked: Vec<CheckOutput>,
    stats: CheckStats,
}

impl WalkVisitor<(Node, Option<CheckData>, Option<StepStats>)> for ValidatingVisitor {
    fn visit(
        &self,
        current: ResolvedNode,
        outgoing: Vec<OutgoingEdge>,
    ) -> (
        (Node, Option<CheckData>, Option<StepStats>),
        Vec<OutgoingEdge>,
    ) {
        let checks_to_do: Option<&HashSet<_>> =
            self.checks_by_node_type.get(&current.node.get_type());
        // The incoming resolved edge counts as one
        let mut num_edges: u64 = 1;
        let mut pass = 0;
        let mut fail = 0;
        let checked: Vec<_> = checks_to_do
            .map(|set| {
                set.iter().filter_map(|check| {
                    // Lets check!
                    let status = match check {
                        CheckType::HgLinkNodePopulated => {
                            num_edges += outgoing.len() as u64;
                            check_linknode_populated(&outgoing)
                        }
                    };
                    if status == CheckStatus::Pass {
                        pass += 1;
                    } else {
                        fail += 1;
                    }
                    Some(CheckOutput::new(*check, status))
                })
            })
            .into_iter()
            .flatten()
            .collect();

        STATS::walker_validate.add_value(
            num_edges as i64,
            (self.repo_stats_key.clone(), EDGES, TOTAL),
        );

        // Call inner after checks. otherwise it will prune outgoing edges we wanted to check.
        let ((node, _opt_data, opt_stats), outgoing) = self.inner.visit(current, outgoing);

        let vout = (
            node,
            if checked.is_empty() {
                None
            } else {
                Some(CheckData {
                    checked,
                    stats: CheckStats {
                        pass,
                        fail,
                        edges: num_edges,
                    },
                })
            },
            opt_stats,
        );
        (vout, outgoing)
    }
}

fn parse_check_types(sub_m: &ArgMatches<'_>) -> Result<HashSet<CheckType>, Error> {
    let mut include_types: HashSet<CheckType> = match sub_m.values_of(INCLUDE_CHECK_TYPE_ARG) {
        None => Ok(HashSet::from_iter(DEFAULT_CHECK_TYPES.iter().cloned())),
        Some(values) => values.map(CheckType::from_str).collect(),
    }?;
    let exclude_types: HashSet<CheckType> = match sub_m.values_of(EXCLUDE_CHECK_TYPE_ARG) {
        None => Ok(HashSet::new()),
        Some(values) => values.map(CheckType::from_str).collect(),
    }?;
    include_types.retain(|x| !exclude_types.contains(x));
    Ok(include_types)
}

struct ValidateProgressState {
    logger: Logger,
    fb: FacebookInit,
    repo_stats_key: String,
    types_sorted_by_name: Vec<CheckType>,
    stats_by_type: HashMap<CheckType, CheckStats>,
    total_checks: CheckStats,
    checked_nodes: u64,
    passed_nodes: u64,
    failed_nodes: u64,
    throttle_reporting_rate: u64,
    throttle_duration: Duration,
    last_update: Instant,
}

impl ValidateProgressState {
    fn new(
        logger: Logger,
        fb: FacebookInit,
        repo_stats_key: String,
        included_types: HashSet<CheckType>,
        sample_rate: u64,
        throttle_duration: Duration,
    ) -> Self {
        let types_sorted_by_name = sort_by_string(included_types);
        let now = Instant::now();
        Self {
            logger,
            fb,
            repo_stats_key,
            types_sorted_by_name,
            stats_by_type: HashMap::new(),
            total_checks: CheckStats::default(),
            checked_nodes: 0,
            passed_nodes: 0,
            failed_nodes: 0,
            throttle_reporting_rate: sample_rate,
            throttle_duration,
            last_update: now,
        }
    }

    fn report_progress_log(&self) {
        let detail_by_type = &self
            .types_sorted_by_name
            .iter()
            .map(|t| {
                let d = CheckStats::default();
                let stats = self.stats_by_type.get(t).unwrap_or(&d);
                format!("{}:{},{}", t, stats.pass, stats.fail)
            })
            .collect::<Vec<_>>()
            .join(" ");
        info!(
            self.logger,
            "Nodes,Pass,Fail:{},{},{}; EdgesChecked:{}; CheckType:Pass,Fail Total:{},{} {}",
            self.checked_nodes,
            self.passed_nodes,
            self.failed_nodes,
            self.total_checks.edges,
            self.total_checks.pass,
            self.total_checks.fail,
            detail_by_type,
        );
    }

    fn report_progress_stats(&self) {
        let service_data = get_service_data_singleton(self.fb);
        // Per check type
        for (k, v) in self.stats_by_type.iter() {
            for (desc, value) in &[(PASS, v.pass), (FAIL, v.fail), (EDGES, v.edges)] {
                service_data.set_counter(
                    &format!(
                        "{}.{}.{}.last_completed.{}",
                        STATS_PREFIX,
                        self.repo_stats_key,
                        k.stats_key(),
                        desc,
                    ),
                    *value as i64,
                );
            }
        }
        // Overall by nodes and edges
        for (stat, desc, value) in &[
            (NODES, PASS, self.passed_nodes),
            (NODES, FAIL, self.failed_nodes),
            (NODES, TOTAL, self.checked_nodes),
            (EDGES, TOTAL, self.total_checks.edges),
        ] {
            service_data.set_counter(
                &format!(
                    "{}.{}.{}.last_completed.{}",
                    STATS_PREFIX, self.repo_stats_key, stat, desc,
                ),
                *value as i64,
            );
        }
    }
}

impl ProgressRecorderUnprotected<CheckData> for ValidateProgressState {
    fn record_step(self: &mut Self, n: &Node, opt: Option<&CheckData>) {
        self.checked_nodes += 1;
        let mut had_pass = false;
        let mut had_fail = false;
        opt.map(|checkdata| {
            // By node. One fail is enough for a Node to be failed.
            if checkdata.stats.fail > 0 {
                had_fail = true;
            } else if !had_fail && checkdata.stats.pass > 0 {
                had_pass = true;
            }
            // total
            self.total_checks += checkdata.stats;
            // By type
            for c in &checkdata.checked {
                let k = c.check;
                let stats = self.stats_by_type.entry(k).or_insert(CheckStats::default());
                if c.status == CheckStatus::Pass {
                    stats.pass += 1;
                    STATS::walker_validate
                        .add_value(1, (self.repo_stats_key.clone(), k.stats_key(), PASS));
                } else {
                    STATS::walker_validate
                        .add_value(1, (self.repo_stats_key.clone(), k.stats_key(), FAIL));
                    stats.fail += 1;
                    // For failures log immediately as well as reporting via stats
                    warn!(
                        self.logger,
                        "Validation failed: check_type,node, {:?},{:?}", k, n
                    )
                }
            }
        });

        if had_pass {
            self.passed_nodes += 1;
            STATS::walker_validate.add_value(1, (self.repo_stats_key.clone(), NODES, PASS));
        } else if had_fail {
            self.failed_nodes += 1;
            STATS::walker_validate.add_value(1, (self.repo_stats_key.clone(), NODES, FAIL));
        }
        STATS::walker_validate.add_value(1, (self.repo_stats_key.clone(), NODES, TOTAL));
    }
}

impl ProgressReporterUnprotected for ValidateProgressState {
    fn report_progress(self: &mut Self) {
        self.report_progress_log();
        self.report_progress_stats();
    }

    fn report_throttled(self: &mut Self) -> Option<Duration> {
        if self.checked_nodes % self.throttle_reporting_rate == 0 {
            let new_update = Instant::now();
            let delta_time = new_update.duration_since(self.last_update);
            if delta_time >= self.throttle_duration {
                self.report_progress_log();
                self.last_update = new_update;
            }
            Some(delta_time)
        } else {
            None
        }
    }
}

// Subcommand entry point for validation of mononoke commit graph and dependent data
pub fn validate(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    let (blobrepo, walk_params) = try_boxfuture!(setup_common(fb, &logger, matches, sub_m));
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let repo_stats_key = try_boxfuture!(args::get_repo_name(fb, &matches));

    cloned!(
        walk_params.include_node_types,
        walk_params.include_edge_types,
    );

    let mut include_check_types = try_boxfuture!(parse_check_types(sub_m));
    include_check_types.retain(|t| include_node_types.contains(&t.node_type()));

    let progress_state = ProgressStateMutex::new(ProgressStateCountByType::new(
        logger.clone(),
        "validate",
        repo_stats_key.clone(),
        walk_params.progress_node_types(),
        PROGRESS_SAMPLE_RATE,
        Duration::from_secs(PROGRESS_SAMPLE_DURATION_S),
    ));

    info!(
        logger,
        "Performing check types {:?}",
        sort_by_string(&include_check_types)
    );

    let stateful_visitor = WalkState::new(ValidatingVisitor::new(
        repo_stats_key.clone(),
        include_node_types,
        include_edge_types,
        include_check_types.clone(),
    ));

    let validate_progress_state = ProgressStateMutex::new(ValidateProgressState::new(
        logger.clone(),
        ctx.fb,
        repo_stats_key,
        include_check_types,
        PROGRESS_SAMPLE_RATE,
        Duration::from_secs(VALIDATE_PROGRESS_SAMPLE_DURATION_S),
    ));

    let make_sink = {
        cloned!(ctx, walk_params.quiet);
        move |walk_output| {
            cloned!(ctx, progress_state, validate_progress_state);
            let walk_progress =
                progress_stream(quiet, progress_state.clone(), walk_output).map(|(n, d, s)| {
                    // swap stats and data round
                    (n, s, d)
                });

            let validate_progress =
                progress_stream(quiet, validate_progress_state.clone(), walk_progress);

            let one_fut = report_state(ctx.clone(), progress_state, validate_progress).map({
                cloned!(validate_progress_state);
                move |d| {
                    validate_progress_state.report_progress();
                    d
                }
            });
            one_fut
        }
    };

    walk_exact_tail(ctx, walk_params, stateful_visitor, blobrepo, make_sink).boxify()
}
