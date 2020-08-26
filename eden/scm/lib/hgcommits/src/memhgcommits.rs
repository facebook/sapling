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
use dag::ops::DagAddHeads;
use dag::MemDag;
use dag::Set;
use dag::Vertex;
use minibytes::Bytes;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;

/// HG commits in memory.
pub struct MemHgCommits {
    commits: HashMap<Vertex, Bytes>,
    dag: MemDag,
}

impl MemHgCommits {
    pub fn new() -> Result<Self> {
        let result = Self {
            dag: MemDag::new(),
            commits: HashMap::new(),
        };
        Ok(result)
    }
}

impl AppendCommits for MemHgCommits {
    fn add_commits(&mut self, commits: &[HgCommit]) -> Result<()> {
        // Write commit data to zstore.
        for commit in commits {
            self.commits
                .insert(commit.vertex.clone(), commit.raw_text.clone());
        }

        // Write commit graph to DAG.
        let commits: HashMap<Vertex, HgCommit> = commits
            .iter()
            .cloned()
            .map(|c| (c.vertex.clone(), c))
            .collect();
        let parent_func = |v: Vertex| -> dag::Result<Vec<Vertex>> {
            match commits.get(&v) {
                Some(commit) => Ok(commit.parents.clone()),
                None => v.not_found(),
            }
        };
        let heads: Vec<Vertex> = {
            let mut heads: HashSet<Vertex> = commits.keys().cloned().collect();
            for commit in commits.values() {
                for parent in commit.parents.iter() {
                    heads.remove(parent);
                }
            }
            heads.into_iter().collect()
        };
        self.dag.add_heads(parent_func, &heads)?;

        Ok(())
    }

    fn flush(&mut self, _master_heads: &[Vertex]) -> Result<()> {
        Ok(())
    }
}

impl ReadCommitText for MemHgCommits {
    fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        Ok(self.commits.get(vertex).cloned())
    }
}

impl StripCommits for MemHgCommits {
    fn strip_commits(&mut self, set: Set) -> Result<()> {
        let mut new = Self::new()?;
        strip::migrate_commits(self, &mut new, set)?;
        *self = new;
        Ok(())
    }
}

delegate!(IdConvert | PrefixLookup | DagAlgorithm | ToIdSet | ToSet, MemHgCommits => self.dag);

impl DescribeBackend for MemHgCommits {
    fn algorithm_backend(&self) -> &'static str {
        "segments"
    }

    fn describe_backend(&self) -> String {
        r#"Backend (memory):
  Local:
    Memory
Feature Providers:
  Commit Graph Algorithms:
    Memory
  Commit Hash / Rev Lookup:
    Memory
  Commit Data (user, message):
    Memory
"#
        .to_string()
    }

    fn explain_internals(&self, w: &mut dyn io::Write) -> io::Result<()> {
        write!(w, "{:?}", &self.dag)
    }
}
