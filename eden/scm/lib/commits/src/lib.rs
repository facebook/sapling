/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![allow(unexpected_cfgs)]

//! # commits
//!
//! Commits stored in HG format and backed by efficient `dag` structures.

pub use commits_trait::AppendCommits;
pub use commits_trait::DagCommits;
pub use commits_trait::DescribeBackend;
pub use commits_trait::GraphNode;
pub use commits_trait::HgCommit;
pub use commits_trait::NewCommit;
pub use commits_trait::ParentlessHgCommit;
pub use commits_trait::ReadCommitText;
pub use commits_trait::StreamCommitText;
pub use commits_trait::StripCommits;
pub use commits_trait::trait_impls;

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
pub use format_util::CommitFields;
pub use hybrid::HybridCommits;
pub use mem_commits::MemCommits;
pub use on_disk_commits::OnDiskCommits;
pub use revlog::RevlogCommits;

impl DagCommits for OnDiskCommits {}
impl DagCommits for HybridCommits {}
impl DagCommits for MemCommits {}
impl DagCommits for RevlogCommits {}
impl DagCommits for DoubleWriteCommits {}

pub async fn add_new_commit(
    dag: &mut (impl DagCommits + ?Sized),
    new_commit: NewCommit,
) -> Result<types::HgId> {
    match dag.format() {
        storemodel::SerializationFormat::Hg => {
            let parents: Vec<_> = new_commit
                .parents
                .iter()
                .filter(|p| !p.is_null())
                .map(|p| dag::Vertex::copy_from(p.as_ref()))
                .collect();

            let (text_bytes, node) = new_commit.into_hg_text_node_pair()?;
            let vertex = dag::Vertex::copy_from(node.as_ref());

            let hg_commit = HgCommit {
                vertex,
                parents,
                raw_text: text_bytes.into(),
            };

            dag.add_commits(&[hg_commit]).await?;
            Ok(node)
        }
        storemodel::SerializationFormat::Git => {
            anyhow::bail!("Git commit creation is not yet supported")
        }
    }
}

/// Initialization. Register abstraction implementations.
pub fn init() {
    factory_impls::setup_commits_constructor();
}
