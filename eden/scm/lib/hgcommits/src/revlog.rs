/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::strip;
use crate::AppendCommits;
use crate::DescribeBackend;
use crate::HgCommit;
use crate::ReadCommitText;
use crate::Result;
use crate::StripCommits;
use dag::delegate;
use dag::ops::IdConvert;
use dag::Group;
use dag::Set;
use dag::Vertex;
use minibytes::Bytes;
use revlogindex::RevlogIndex;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

/// HG commits stored on disk using the revlog format.
pub struct RevlogCommits {
    revlog: RevlogIndex,
    pub(crate) dir: PathBuf,
}

impl RevlogCommits {
    pub fn new(dir: &Path) -> Result<Self> {
        let index_path = dir.join("00changelog.i");
        let nodemap_path = dir.join("00changelog.nodemap");
        let revlog = RevlogIndex::new(&index_path, &nodemap_path)?;
        Ok(Self {
            revlog,
            dir: dir.to_path_buf(),
        })
    }
}

impl AppendCommits for RevlogCommits {
    fn add_commits(&mut self, commits: &[HgCommit]) -> Result<()> {
        for commit in commits {
            let mut parent_revs = Vec::with_capacity(commit.parents.len());
            for parent in &commit.parents {
                parent_revs.push(self.revlog.vertex_id(parent.clone())?.0 as u32);
            }
            self.revlog
                .insert(commit.vertex.clone(), parent_revs, commit.raw_text.clone())
        }
        Ok(())
    }

    fn flush(&mut self, _master_heads: &[Vertex]) -> Result<()> {
        self.revlog.flush()?;
        Ok(())
    }
}

impl ReadCommitText for RevlogCommits {
    fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        match self.vertex_id_with_max_group(vertex, Group::NON_MASTER)? {
            Some(id) => Ok(Some(self.revlog.raw_data(id.0 as u32)?)),
            None => Ok(None),
        }
    }
}

impl StripCommits for RevlogCommits {
    fn strip_commits(&mut self, set: Set) -> Result<()> {
        let old_dir = &self.dir;
        let new_dir = old_dir.join("strip");
        let _ = fs::create_dir(&new_dir);
        let mut new = Self::new(&new_dir)?;
        strip::migrate_commits(self, &mut new, set)?;
        drop(new);
        strip::racy_unsafe_move_files(&new_dir, old_dir)?;
        *self = Self::new(old_dir)?;
        Ok(())
    }
}

delegate!(IdConvert | PrefixLookup | DagAlgorithm | ToIdSet | ToSet, RevlogCommits => self.revlog);

impl DescribeBackend for RevlogCommits {
    fn algorithm_backend(&self) -> &'static str {
        "revlog"
    }

    fn describe_backend(&self) -> String {
        format!(
            r#"Backend (revlog):
  Local:
    Revlog: {}
    Nodemap: {}
Feature Providers:
  Commit Graph Algorithms:
    Revlog
  Commit Hash / Rev Lookup:
    Nodemap
  Commit Data (user, message):
    Revlog
"#,
            self.dir.join("00changelog.{i,d}").display(),
            self.dir.join("00changelog.nodemap").display(),
        )
    }

    fn explain_internals(&self, w: &mut dyn io::Write) -> io::Result<()> {
        writeln!(w, "(RevlogIndex explain_internals is not yet implemented)")
    }
}
