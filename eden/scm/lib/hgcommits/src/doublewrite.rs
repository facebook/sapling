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
use crate::ReadCommitText;
use crate::RevlogCommits;
use crate::StripCommits;
use anyhow::Result;
use dag::ops::DagAlgorithm;
use dag::ops::IdConvert;
use dag::ops::PrefixLookup;
use dag::ops::ToIdSet;
use dag::ops::ToSet;
use dag::Group;
use dag::Id;
use dag::IdSet;
use dag::Set;
use dag::Vertex;
use minibytes::Bytes;
use std::path::Path;
use std::sync::Arc;

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
}

impl ReadCommitText for DoubleWriteCommits {
    fn get_commit_raw_text(&self, vertex: &Vertex) -> Result<Option<Bytes>> {
        match self.commits.get_commit_raw_text(vertex) {
            Ok(None) => self.revlog.get_commit_raw_text(vertex),
            result => result,
        }
    }
}

impl StripCommits for DoubleWriteCommits {
    fn strip_commits(&mut self, set: Set) -> Result<()> {
        self.revlog.strip_commits(set.clone())?;
        self.commits.strip_commits(set)?;
        Ok(())
    }
}

impl IdConvert for DoubleWriteCommits {
    fn vertex_id(&self, name: Vertex) -> dag::Result<Id> {
        self.commits.vertex_id(name)
    }
    fn vertex_id_with_max_group(&self, name: &Vertex, max_group: Group) -> dag::Result<Option<Id>> {
        self.commits.vertex_id_with_max_group(name, max_group)
    }
    fn vertex_name(&self, id: Id) -> dag::Result<Vertex> {
        self.commits.vertex_name(id)
    }
    fn contains_vertex_name(&self, name: &Vertex) -> dag::Result<bool> {
        self.commits.contains_vertex_name(name)
    }
}

impl PrefixLookup for DoubleWriteCommits {
    fn vertexes_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> dag::Result<Vec<Vertex>> {
        self.commits.vertexes_by_hex_prefix(hex_prefix, limit)
    }
}

impl DagAlgorithm for DoubleWriteCommits {
    fn sort(&self, set: &Set) -> dag::Result<Set> {
        self.commits.sort(set)
    }
    fn parent_names(&self, name: Vertex) -> dag::Result<Vec<Vertex>> {
        self.commits.parent_names(name)
    }
    fn all(&self) -> dag::Result<Set> {
        self.commits.all()
    }
    fn ancestors(&self, set: Set) -> dag::Result<Set> {
        self.commits.ancestors(set)
    }
    fn parents(&self, set: Set) -> dag::Result<Set> {
        self.commits.parents(set)
    }
    fn first_ancestor_nth(&self, name: Vertex, n: u64) -> dag::Result<Vertex> {
        self.commits.first_ancestor_nth(name, n)
    }
    fn heads(&self, set: Set) -> dag::Result<Set> {
        self.commits.heads(set)
    }
    fn children(&self, set: Set) -> dag::Result<Set> {
        self.commits.children(set)
    }
    fn roots(&self, set: Set) -> dag::Result<Set> {
        self.commits.roots(set)
    }
    fn gca_one(&self, set: Set) -> dag::Result<Option<Vertex>> {
        self.commits.gca_one(set)
    }
    fn gca_all(&self, set: Set) -> dag::Result<Set> {
        self.commits.gca_all(set)
    }
    fn common_ancestors(&self, set: Set) -> dag::Result<Set> {
        self.commits.common_ancestors(set)
    }
    fn is_ancestor(&self, ancestor: Vertex, descendant: Vertex) -> dag::Result<bool> {
        self.commits.is_ancestor(ancestor, descendant)
    }
    fn heads_ancestors(&self, set: Set) -> dag::Result<Set> {
        self.commits.heads_ancestors(set)
    }
    fn range(&self, roots: Set, heads: Set) -> dag::Result<Set> {
        self.commits.range(roots, heads)
    }
    fn only(&self, reachable: Set, unreachable: Set) -> dag::Result<Set> {
        self.commits.only(reachable, unreachable)
    }
    fn only_both(&self, reachable: Set, unreachable: Set) -> dag::Result<(Set, Set)> {
        self.commits.only_both(reachable, unreachable)
    }
    fn descendants(&self, set: Set) -> dag::Result<Set> {
        self.commits.descendants(set)
    }
    fn reachable_roots(&self, roots: Set, heads: Set) -> dag::Result<Set> {
        self.commits.reachable_roots(roots, heads)
    }
    fn snapshot_dag(&self) -> dag::Result<Arc<dyn DagAlgorithm + Send + Sync>> {
        self.commits.snapshot_dag()
    }
}

impl ToIdSet for DoubleWriteCommits {
    fn to_id_set(&self, set: &Set) -> dag::Result<IdSet> {
        self.commits.to_id_set(set)
    }
}

impl ToSet for DoubleWriteCommits {
    fn to_set(&self, set: &IdSet) -> dag::Result<Set> {
        self.commits.to_set(set)
    }
}

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
}
