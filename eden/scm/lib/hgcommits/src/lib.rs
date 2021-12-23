/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # hgcommits
//!
//! Commits stored in HG format and backed by efficient `dag` structures.

use std::io;
use std::sync::Arc;

use dag::errors::NotFoundError;
use dag::CloneData;
use dag::Vertex;
use dag::VertexListWithOptions;
use futures::future::try_join_all;
use futures::stream::BoxStream;
use metalog::MetaLog;
use minibytes::Bytes;
use serde::Deserialize;
use serde::Serialize;

#[async_trait::async_trait]
pub trait ReadCommitText: Sync {
    /// Read raw text for a commit.
    async fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        let list = self.get_commit_raw_text_list(&[vertex.clone()]).await?;
        Ok(Some(list.into_iter().next().unwrap()))
    }

    /// Read commit text in batch. Any of the missing commits would cause an error.
    async fn get_commit_raw_text_list(&self, vertexes: &[Vertex]) -> Result<Vec<Bytes>> {
        try_join_all(vertexes.iter().map(|v| async move {
            match self.get_commit_raw_text(v).await {
                Err(e) => Err(e),
                Ok(None) => v.not_found().map_err(|e| e.into()),
                Ok(Some(b)) => Ok(b),
            }
        }))
        .await
    }

    /// Return a trait object to resolve root tree ids from commit ids.
    fn to_dyn_read_commit_text(&self) -> Arc<dyn ReadCommitText + Send + Sync>;
}

pub trait StreamCommitText {
    /// Get commit raw text in a stream fashion.
    fn stream_commit_raw_text(
        &self,
        stream: BoxStream<'static, anyhow::Result<Vertex>>,
    ) -> Result<BoxStream<'static, anyhow::Result<ParentlessHgCommit>>>;
}

#[async_trait::async_trait]
pub trait AppendCommits: Send + Sync {
    /// Add commits. They stay in-memory until `flush`.
    async fn add_commits(&mut self, commits: &[HgCommit]) -> Result<()>;

    /// Write in-memory changes to disk.
    ///
    /// This function does more things than `flush_commit_data`.
    async fn flush(&mut self, master_heads: &[Vertex]) -> Result<()>;

    /// Write buffered commit data to disk.
    ///
    /// For the revlog backend, this also write the commit graph to disk.
    async fn flush_commit_data(&mut self) -> Result<()>;

    /// Add nodes to the graph without data (commit message).
    /// This is only supported by lazy backends.
    /// Use `flush` to write changes to disk.
    async fn add_graph_nodes(&mut self, graph_nodes: &[GraphNode]) -> Result<()> {
        let _ = graph_nodes;
        Err(crate::Error::Unsupported(
            "add_graph_nodes is not supported by this backend",
        ))
    }

    /// Import clone data and flush.
    /// This is only supported by lazy backends and can only be used in an empty repo.
    async fn import_clone_data(&mut self, clone_data: CloneData<Vertex>) -> Result<()> {
        let _ = clone_data;
        Err(crate::Error::Unsupported(
            "import_clone_data is not supported by this backend",
        ))
    }

    /// Import data from master fast forward pull.
    /// This is only supported by lazy backends. Can be used on non-empty repo.
    async fn import_pull_data(
        &mut self,
        clone_data: CloneData<Vertex>,
        heads: &VertexListWithOptions,
    ) -> Result<()> {
        let _ = (clone_data, heads);
        Err(crate::Error::Unsupported(
            "import_pull_data is not supported by this backend",
        ))
    }

    /// Update references to match metalog.
    ///
    /// This is not needed if metalog is the source of truth.
    /// However, if metalog is synced from git references, then this
    /// method is needed to sync metalog back to git references.
    fn update_references_to_match_metalog(&mut self, metalog: &MetaLog) -> Result<()> {
        let _ = metalog;
        Ok(())
    }
}

pub trait DescribeBackend {
    /// Name of the DagAlgorithm backend.
    fn algorithm_backend(&self) -> &'static str;

    /// Describe what storage backend is being used.
    fn describe_backend(&self) -> String;

    /// Write human-readable internal data to `w`.
    /// For segments backend, this writes segments data.
    fn explain_internals(&self, w: &mut dyn io::Write) -> io::Result<()>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphNode {
    pub vertex: Vertex,
    pub parents: Vec<Vertex>,
}

/// Parameter used by `add_commits`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HgCommit {
    pub vertex: Vertex,
    pub parents: Vec<Vertex>,
    pub raw_text: Bytes,
}

/// Return type used by `stream_commit_raw_text`.
#[derive(Serialize, Deserialize, Debug)]
pub struct ParentlessHgCommit {
    pub vertex: Vertex,
    pub raw_text: Bytes,
}

mod doublewrite;
pub(crate) mod errors;
mod git;
mod hgsha1commits;
mod hybrid;
mod memhgcommits;
mod revlog;
mod strip;
mod utils;

pub use doublewrite::DoubleWriteCommits;
pub use errors::CommitError as Error;
pub use git::GitSegmentedCommits;
pub use hgsha1commits::HgCommits;
pub use hybrid::HybridCommits;
pub use memhgcommits::MemHgCommits;
pub use revlog::RevlogCommits;
pub use strip::StripCommits;
pub type Result<T> = std::result::Result<T, Error>;
