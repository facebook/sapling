/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # hgcommits
//!
//! Commits stored in HG format and backed by efficient `dag` structures.

use anyhow::Result;
use dag::Vertex;
use minibytes::Bytes;
use serde::Deserialize;
use serde::Serialize;

pub trait ReadCommitText {
    /// Read raw text for a commit.
    fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>>;
}

pub trait AppendCommits {
    /// Add commits. They stay in-memory.
    fn add_commits(&mut self, commits: &[HgCommit]) -> Result<()>;

    /// Write in-memory changes to disk.
    fn flush(&mut self, master_heads: &[Vertex]) -> Result<()>;
}

/// Parameter used by `add_commits`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HgCommit {
    pub vertex: Vertex,
    pub parents: Vec<Vertex>,
    pub raw_text: Bytes,
}

mod doublewrite;
mod hgsha1commits;
mod memhgcommits;
mod revlog;
mod strip;

pub use doublewrite::DoubleWriteCommits;
pub use hgsha1commits::HgCommits;
pub use memhgcommits::MemHgCommits;
pub use revlog::RevlogCommits;
pub use strip::StripCommits;
