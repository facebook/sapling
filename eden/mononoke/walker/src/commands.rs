/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::detail::graph::EdgeType;
use crate::detail::graph::NodeType;
use crate::detail::progress::ProgressStateCountByType;
use crate::detail::progress::ProgressStateMutex;
use crate::detail::progress::ProgressSummary;
use crate::detail::state::StepStats;
use crate::detail::tail::TailParams;
use crate::detail::walk::RepoWalkParams;

use std::collections::HashSet;

pub const SCRUB: &str = "scrub";
pub const COMPRESSION_BENEFIT: &str = "compression-benefit";
pub const VALIDATE: &str = "validate";
pub const CORPUS: &str = "corpus";

// Per repo things we don't pass into the walk
#[derive(Clone)]
pub struct RepoSubcommandParams {
    pub progress_state: ProgressStateMutex<ProgressStateCountByType<StepStats, ProgressSummary>>,
    pub tail_params: TailParams,
    pub lfs_threshold: Option<u64>,
}

// These don't vary per repo
#[derive(Clone)]
pub struct JobWalkParams {
    pub enable_derive: bool,
    pub quiet: bool,
    pub error_as_data_node_types: HashSet<NodeType>,
    pub error_as_data_edge_types: HashSet<EdgeType>,
    pub repo_count: usize,
}

#[derive(Clone)]
pub struct JobParams {
    pub walk_params: JobWalkParams,
    pub per_repo: Vec<(RepoSubcommandParams, RepoWalkParams)>,
}

mononoke_app::subcommands! {
    mod compression_benefit;
    mod corpus;
    mod scrub;
    mod validate;
}
