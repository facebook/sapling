/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// This module allows the implementation of validating checks over the mononoke graph
// Currently checks are added by
//  1. Add a CheckType variant
//  2. Add CheckType::node_type() and CheckType::enum_type() cases for the new variant
//  3. Add a new validation method
//  4. Add the method to the match/case in ValidatingVisitor::visit()

use crate::graph::{EdgeType, Node, NodeData, NodeType};
use crate::progress::{
    progress_stream, report_state, sort_by_string, ProgressRecorder, ProgressRecorderUnprotected,
    ProgressReporter, ProgressReporterUnprotected, ProgressStateMutex,
};
use crate::setup::{
    setup_common, EXCLUDE_CHECK_TYPE_ARG, INCLUDE_CHECK_TYPE_ARG, PROGRESS_SAMPLE_DURATION_S,
    PROGRESS_SAMPLE_RATE, VALIDATE,
};
use crate::state::{StepStats, WalkState, WalkStateCHashMap};
use crate::tail::{walk_exact_tail, RepoWalkRun};
use crate::walk::{OutgoingEdge, ResolvedNode, WalkVisitor};

use anyhow::{format_err, Error};
use clap::ArgMatches;
use cloned::cloned;
use cmdlib::args;
use fbinit::FacebookInit;
use futures_preview::{
    future::{self, BoxFuture, FutureExt},
    stream::TryStreamExt,
};
use itertools::Itertools;
use mononoke_types::MPath;
use phases::Phase;
use scuba_ext::ScubaSampleBuilder;
use slog::{info, warn, Logger};
use stats::prelude::*;
use stats_facebook::service_data::{get_service_data_singleton, ServiceData};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    hash::Hash,
    iter::FromIterator,
    ops::AddAssign,
    result::Result,
    str::FromStr,
    time::{Duration, Instant},
};

pub const STATS_PREFIX: &'static str = "mononoke.walker.validate";
pub const NODES: &'static str = "nodes";
pub const EDGES: &'static str = "edges";
pub const PASS: &'static str = "pass";
pub const FAIL: &'static str = "fail";
pub const TOTAL: &'static str = "total";
pub const NODE_KEY: &'static str = "node_key";
pub const NODE_TYPE: &'static str = "node_type";
pub const NODE_PATH: &'static str = "node_path";
pub const EDGE_TYPE: &'static str = "edge_type";
pub const CHECK_TYPE: &'static str = "check_type";
pub const CHECK_FAIL: &'static str = "check_fail";
pub const WALK_TYPE: &'static str = "walk_type";
pub const REPO: &'static str = "repo";
const SRC_NODE_KEY: &'static str = "src_node_key";
const SRC_NODE_TYPE: &'static str = "src_node_type";
const SRC_NODE_PATH: &'static str = "src_node_path";

define_stats! {
    prefix = "mononoke.walker.validate";
    // e.g. mononoke.walker.validate.testrepo.hg_link_node_populated.pass
    walker_validate: dynamic_timeseries("{}.{}.{}", (repo: String, check: &'static str, status: &'static str); Rate, Sum),
}

pub const DEFAULT_CHECK_TYPES: &[CheckType] = &[
    CheckType::BonsaiChangesetPhaseIsPublic,
    CheckType::HgLinkNodePopulated,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum CheckStatus {
    Fail,
    Pass,
}

define_type_enum! {
enum CheckType {
    BonsaiChangesetPhaseIsPublic,
    HgLinkNodePopulated,
}
}

impl CheckType {
    fn stats_key(&self) -> &'static str {
        match self {
            CheckType::BonsaiChangesetPhaseIsPublic => "bonsai_phase_is_public",
            CheckType::HgLinkNodePopulated => "hg_link_node_populated",
        }
    }
    pub fn node_type(&self) -> NodeType {
        match self {
            CheckType::BonsaiChangesetPhaseIsPublic => NodeType::BonsaiPhaseMapping,
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

fn check_bonsai_phase_is_public(current: &ResolvedNode) -> CheckStatus {
    match &current.data {
        NodeData::BonsaiPhaseMapping(Some(Phase::Public)) => CheckStatus::Pass,
        _ => CheckStatus::Fail,
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
    source_node: Option<Node>,
    checked: Vec<CheckOutput>,
    stats: CheckStats,
}

impl WalkVisitor<(Node, Option<CheckData>, Option<StepStats>), Node> for ValidatingVisitor {
    fn visit(
        &self,
        current: ResolvedNode,
        route: Option<Node>,
        outgoing: Vec<OutgoingEdge>,
    ) -> (
        (Node, Option<CheckData>, Option<StepStats>),
        Node,
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
                        CheckType::BonsaiChangesetPhaseIsPublic => {
                            check_bonsai_phase_is_public(&current)
                        }
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
        let ((node, _opt_data, opt_stats), _, outgoing) =
            self.inner
                .visit(current, route.as_ref().map(|_| ()), outgoing);

        let vout = (
            node.clone(),
            if checked.is_empty() {
                None
            } else {
                Some(CheckData {
                    source_node: route,
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
        let next_via = node;
        (vout, next_via, outgoing)
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
    scuba_builder: ScubaSampleBuilder,
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
        scuba_builder: ScubaSampleBuilder,
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
            scuba_builder,
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

fn scuba_log_node(
    n: &Node,
    scuba: &mut ScubaSampleBuilder,
    type_key: &'static str,
    key_key: &'static str,
    path_key: &'static str,
) {
    scuba
        .add(type_key, n.get_type().to_string())
        .add(key_key, n.stats_key());
    if let Some(path) = n.stats_path() {
        scuba.add(path_key, MPath::display_opt(path).to_string());
    }
}

pub fn add_node_to_scuba(source_node: Option<&Node>, n: &Node, scuba: &mut ScubaSampleBuilder) {
    scuba_log_node(n, scuba, NODE_TYPE, NODE_KEY, NODE_PATH);
    if let Some(src_node) = source_node {
        scuba_log_node(src_node, scuba, SRC_NODE_TYPE, SRC_NODE_KEY, SRC_NODE_PATH);
    }
}

impl ProgressRecorderUnprotected<CheckData> for ValidateProgressState {
    fn set_sample_builder(&mut self, s: ScubaSampleBuilder) {
        self.scuba_builder = s;
    }

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
            let source_node = &checkdata.source_node;
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
                    // For failures log immediately
                    let mut scuba = self.scuba_builder.clone();
                    add_node_to_scuba(source_node.as_ref(), n, &mut scuba);
                    scuba
                        .add(CHECK_TYPE, k.stats_key())
                        .add(
                            CHECK_FAIL,
                            if c.status == CheckStatus::Pass { 0 } else { 1 },
                        )
                        .log();
                    for json in scuba.get_sample().to_json() {
                        warn!(self.logger, "Validation failed: {}", json)
                    }
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
) -> BoxFuture<'static, Result<(), Error>> {
    match setup_common(VALIDATE, fb, &logger, matches, sub_m).and_then(
        |(datasources, walk_params)| {
            args::get_repo_name(fb, &matches).and_then(|repo_stats_key| {
                parse_check_types(sub_m).and_then(|mut include_check_types| {
                    include_check_types
                        .retain(|t| walk_params.include_node_types.contains(&t.node_type()));
                    Ok((
                        datasources,
                        walk_params,
                        repo_stats_key,
                        include_check_types,
                    ))
                })
            })
        },
    ) {
        Err(e) => future::err::<_, Error>(e).boxed(),
        Ok((datasources, walk_params, repo_stats_key, include_check_types)) => {
            cloned!(
                walk_params.include_node_types,
                walk_params.include_edge_types,
            );
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
                fb,
                datasources.scuba_builder.clone(),
                repo_stats_key,
                include_check_types,
                PROGRESS_SAMPLE_RATE,
                Duration::from_secs(PROGRESS_SAMPLE_DURATION_S),
            ));

            cloned!(walk_params.progress_state, walk_params.quiet);
            let make_sink = move |run: RepoWalkRun| {
                cloned!(run.ctx);
                validate_progress_state.set_sample_builder(run.scuba_builder);
                async move |walk_output| {
                    cloned!(ctx, progress_state, validate_progress_state);
                    let walk_progress =
                        progress_stream(quiet, &progress_state.clone(), walk_output).map_ok(
                            |(n, d, s)| {
                                // swap stats and data round
                                (n, s, d)
                            },
                        );

                    let validate_progress =
                        progress_stream(quiet, &validate_progress_state.clone(), walk_progress);

                    let one_fut =
                        report_state(ctx.clone(), progress_state, validate_progress).map({
                            cloned!(validate_progress_state);
                            move |d| {
                                validate_progress_state.report_progress();
                                d
                            }
                        });
                    one_fut.await
                }
            };
            walk_exact_tail(
                fb,
                logger,
                datasources,
                walk_params,
                stateful_visitor,
                make_sink,
            )
            .boxed()
        }
    }
}
