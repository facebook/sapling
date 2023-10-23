/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # hgcommits
//!
//! Commits stored in HG format and backed by efficient `dag` structures.

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
mod hgsha1commits;
mod hybrid;
mod memhgcommits;
mod revlog;
mod strip;
pub mod trait_impls;
mod utils;

pub use anyhow::Result;
pub use doublewrite::DoubleWriteCommits;
pub use hgsha1commits::HgCommits;
pub use hybrid::HybridCommits;
pub use memhgcommits::MemHgCommits;
pub use revlog::RevlogCommits;

impl DagCommits for HgCommits {}
impl DagCommits for HybridCommits {}
impl DagCommits for MemHgCommits {}
impl DagCommits for RevlogCommits {}
impl DagCommits for DoubleWriteCommits {}

/// Initialization. Register abstraction implementations.
pub fn init() {
    factory_impls::setup_commits_constructor();
}
