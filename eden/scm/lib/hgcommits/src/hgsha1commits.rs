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
use dag::ops::DagAlgorithm;
use dag::ops::DagPersistent;
use dag::Dag;
use dag::Set;
use dag::Vertex;
use minibytes::Bytes;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use zstore::Id20;
use zstore::Zstore;

/// Commits using the HG SHA1 hash function. Stored on disk.
pub struct HgCommits {
    commits: Zstore,
    pub(crate) commits_path: PathBuf,
    dag: Dag,
    pub(crate) dag_path: PathBuf,
}

impl HgCommits {
    pub fn new(dag_path: &Path, commits_path: &Path) -> Result<Self> {
        let result = Self {
            dag: Dag::open(dag_path)?,
            dag_path: dag_path.to_path_buf(),
            commits: Zstore::open(commits_path)?,
            commits_path: commits_path.to_path_buf(),
        };
        Ok(result)
    }

    /// Import another DAG. `main` specifies the main branch for commit graph
    /// optimization.
    pub fn import_dag(&mut self, other: impl DagAlgorithm, main: Set) -> Result<()> {
        self.dag.import_and_flush(&other, main)?;
        Ok(())
    }
}

impl AppendCommits for HgCommits {
    fn add_commits(&mut self, commits: &[HgCommit]) -> Result<()> {
        fn null_id() -> Vertex {
            Vertex::copy_from(Id20::null_id().as_ref())
        }

        // The SHA1 of hg commit includes sorted(p1, p2) as header.
        fn text_with_header(raw_text: &[u8], parents: &[Vertex]) -> Result<Vec<u8>> {
            let mut result = Vec::with_capacity(raw_text.len() + Id20::len() * 2);
            let (p1, p2) = (
                parents.get(0).cloned().unwrap_or_else(null_id),
                parents.get(1).cloned().unwrap_or_else(null_id),
            );
            if p1 < p2 {
                result.write_all(p1.as_ref())?;
                result.write_all(p2.as_ref())?;
            } else {
                result.write_all(p2.as_ref())?;
                result.write_all(p1.as_ref())?;
            }
            result.write_all(&raw_text)?;
            Ok(result)
        }

        // Write commit data to zstore.
        for commit in commits {
            let text = text_with_header(&commit.raw_text, &commit.parents)?;
            let vertex = Vertex::copy_from(self.commits.insert(&text, &[])?.as_ref());
            if vertex != commit.vertex {
                return Err(crate::Error::HashMismatch(vertex, commit.vertex.clone()));
            }
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

    fn flush(&mut self, master_heads: &[Vertex]) -> Result<()> {
        self.commits.flush()?;
        self.dag.flush(master_heads)?;
        Ok(())
    }

    fn flush_commit_data(&mut self) -> Result<()> {
        self.commits.flush()?;
        Ok(())
    }
}

impl ReadCommitText for HgCommits {
    fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        let id = Id20::from_slice(vertex.as_ref())?;
        match self.commits.get(id)? {
            Some(bytes) => Ok(Some(bytes.slice(Id20::len() * 2..))),
            None => Ok(None),
        }
    }
}

impl StripCommits for HgCommits {
    fn strip_commits(&mut self, set: Set) -> Result<()> {
        let old_path = &self.dag_path;
        let new_path = self.dag_path.join("strip");
        let mut new = Self::new(&new_path, &self.commits_path)?;
        strip::migrate_commits(self, &mut new, set)?;
        drop(new);
        strip::racy_unsafe_move_files(&new_path, &self.dag_path)?;
        *self = Self::new(&old_path, &self.commits_path)?;
        Ok(())
    }
}

delegate!(IdConvert | PrefixLookup | DagAlgorithm | ToIdSet | ToSet, HgCommits => self.dag);

impl DescribeBackend for HgCommits {
    fn algorithm_backend(&self) -> &'static str {
        "segments"
    }

    fn describe_backend(&self) -> String {
        format!(
            r#"Backend (non-lazy segments):
  Local:
    Segments + IdMap: {}
    Zstore: {}
Feature Providers:
  Commit Graph Algorithms:
    Segments
  Commit Hash / Rev Lookup:
    IdMap
  Commit Data (user, message):
    Zstore
"#,
            self.dag_path.display(),
            self.commits_path.display()
        )
    }

    fn explain_internals(&self, w: &mut dyn io::Write) -> io::Result<()> {
        write!(w, "{:?}", &self.dag)
    }
}
