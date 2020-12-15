/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # hgcommits
//!
//! Commits stored in HG format and backed by efficient `dag` structures.

use dag::Vertex;
use futures::future::try_join_all;
use futures::stream::BoxStream;
use minibytes::Bytes;
use serde::Deserialize;
use serde::Serialize;
use std::io;

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
}

pub trait StreamCommitText {
    /// Get commit raw text in a stream fashion.
    fn stream_commit_raw_text(
        &self,
        stream: BoxStream<'static, anyhow::Result<Vertex>>,
    ) -> Result<BoxStream<'static, anyhow::Result<ParentlessHgCommit>>>;
}

#[async_trait::async_trait]
pub trait AppendCommits {
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

pub use doublewrite::DoubleWriteCommits;
pub use git::GitSegmentedCommits;
pub use hgsha1commits::HgCommits;
pub use hybrid::HybridCommits;
pub use memhgcommits::MemHgCommits;
pub use revlog::RevlogCommits;
pub use strip::StripCommits;

pub use errors::CommitError as Error;
pub type Result<T> = std::result::Result<T, Error>;
