/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::AppendCommits;
use crate::DescribeBackend;
use crate::HgCommit;
use crate::HgCommits;
use crate::ParentlessHgCommit;
use crate::ReadCommitText;
use crate::Result;
use crate::RevlogCommits;
use crate::StreamCommitText;
use crate::StripCommits;
use dag::delegate;
use dag::Set;
use dag::Vertex;
use futures::stream::BoxStream;
use minibytes::Bytes;
use std::io;
use std::path::Path;

/// Segmented Changelog + Revlog.
///
/// Use segmented changelog for the commit graph algorithms and IdMap.
/// Use revlog for fallback commit messages. Double writes to revlog.
pub struct DoubleWriteCommits {
    revlog: RevlogCommits,
    commits: HgCommits,
}

impl DoubleWriteCommits {
    pub fn new(revlog_dir: &Path, dag_path: &Path, commits_path: &Path) -> Result<Self> {
        let commits = HgCommits::new(dag_path, commits_path)?;
        let revlog = RevlogCommits::new(revlog_dir)?;
        Ok(Self { revlog, commits })
    }
}

impl AppendCommits for DoubleWriteCommits {
    fn add_commits(&mut self, commits: &[HgCommit]) -> Result<()> {
        self.revlog.add_commits(commits)?;
        self.commits.add_commits(commits)?;
        Ok(())
    }

    fn flush(&mut self, master_heads: &[Vertex]) -> Result<()> {
        self.revlog.flush(master_heads)?;
        self.commits.flush(master_heads)?;
        Ok(())
    }

    fn flush_commit_data(&mut self) -> Result<()> {
        self.revlog.flush_commit_data()?;
        self.commits.flush_commit_data()?;
        Ok(())
    }
}

impl ReadCommitText for DoubleWriteCommits {
    fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        match self.commits.get_commit_raw_text(vertex) {
            Ok(None) => self.revlog.get_commit_raw_text(vertex),
            result => result,
        }
    }
}

impl StreamCommitText for DoubleWriteCommits {
    fn stream_commit_raw_text(
        &self,
        stream: BoxStream<'static, anyhow::Result<Vertex>>,
    ) -> Result<BoxStream<'static, anyhow::Result<ParentlessHgCommit>>> {
        self.revlog.stream_commit_raw_text(stream)
    }
}

impl StripCommits for DoubleWriteCommits {
    fn strip_commits(&mut self, set: Set) -> Result<()> {
        self.revlog.strip_commits(set.clone())?;
        self.commits.strip_commits(set)?;
        Ok(())
    }
}

delegate!(IdConvert | IdMapSnapshot | PrefixLookup | DagAlgorithm, DoubleWriteCommits => self.commits);

impl DescribeBackend for DoubleWriteCommits {
    fn algorithm_backend(&self) -> &'static str {
        "segments"
    }

    fn describe_backend(&self) -> String {
        format!(
            r#"Backend (doublewrite):
  Local:
    Segments + IdMap: {}
    Zstore: {}
    Revlog + Nodemap: {}
Feature Providers:
  Commit Graph Algorithms:
    Segments
  Commit Hash / Rev Lookup:
    IdMap
  Commit Data (user, message):
    Zstore (incomplete)
    Revlog
"#,
            self.commits.dag_path.display(),
            self.commits.commits_path.display(),
            self.revlog.dir.join("00changelog.{i,d,nodemap}").display(),
        )
    }

    fn explain_internals(&self, w: &mut dyn io::Write) -> io::Result<()> {
        self.commits.explain_internals(w)
    }
}
