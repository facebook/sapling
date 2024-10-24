/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(unexpected_cfgs)]

//! # commits
//!
//! Commits stored in HG format and backed by efficient `dag` structures.

pub use commits_trait::trait_impls;
pub use commits_trait::AppendCommits;
pub use commits_trait::DagCommits;
pub use commits_trait::DescribeBackend;
pub use commits_trait::GraphNode;
pub use commits_trait::HgCommit;
pub use commits_trait::ParentlessHgCommit;
pub use commits_trait::ReadCommitText;
pub use commits_trait::StreamCommitText;
pub use commits_trait::StripCommits;

mod doublewrite;
pub(crate) mod errors;
mod factory_impls;
mod hybrid;
mod mem_commits;
mod on_disk_commits;
mod revlog;
mod strip;
mod utils;

pub use anyhow::Result;
pub use doublewrite::DoubleWriteCommits;
pub use hybrid::HybridCommits;
pub use mem_commits::MemCommits;
pub use on_disk_commits::OnDiskCommits;
pub use revlog::RevlogCommits;

impl DagCommits for OnDiskCommits {}
impl DagCommits for HybridCommits {}
impl DagCommits for MemCommits {}
impl DagCommits for RevlogCommits {}
impl DagCommits for DoubleWriteCommits {}

/// Initialization. Register abstraction implementations.
pub fn init() {
    factory_impls::setup_commits_constructor();
}
