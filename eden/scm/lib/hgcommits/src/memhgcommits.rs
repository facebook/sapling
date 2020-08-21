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
use crate::StripCommits;
use anyhow::Result;
use dag::ops::DagAddHeads;
use dag::ops::DagAlgorithm;
use dag::ops::IdConvert;
use dag::ops::PrefixLookup;
use dag::ops::ToIdSet;
use dag::ops::ToSet;
use dag::Group;
use dag::Id;
use dag::IdSet;
use dag::MemDag;
use dag::Set;
use dag::Vertex;
use minibytes::Bytes;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

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

impl IdConvert for MemHgCommits {
    fn vertex_id(&self, name: Vertex) -> dag::Result<Id> {
        self.dag.vertex_id(name)
    }
    fn vertex_id_with_max_group(&self, name: &Vertex, max_group: Group) -> dag::Result<Option<Id>> {
        self.dag.vertex_id_with_max_group(name, max_group)
    }
    fn vertex_name(&self, id: Id) -> dag::Result<Vertex> {
        self.dag.vertex_name(id)
    }
    fn contains_vertex_name(&self, name: &Vertex) -> dag::Result<bool> {
        self.dag.contains_vertex_name(name)
    }
}

impl PrefixLookup for MemHgCommits {
    fn vertexes_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> dag::Result<Vec<Vertex>> {
        self.dag.vertexes_by_hex_prefix(hex_prefix, limit)
    }
}

impl DagAlgorithm for MemHgCommits {
    fn sort(&self, set: &Set) -> dag::Result<Set> {
        self.dag.sort(set)
    }
    fn parent_names(&self, name: Vertex) -> dag::Result<Vec<Vertex>> {
        self.dag.parent_names(name)
    }
    fn all(&self) -> dag::Result<Set> {
        self.dag.all()
    }
    fn ancestors(&self, set: Set) -> dag::Result<Set> {
        self.dag.ancestors(set)
    }
    fn parents(&self, set: Set) -> dag::Result<Set> {
        self.dag.parents(set)
    }
    fn first_ancestor_nth(&self, name: Vertex, n: u64) -> dag::Result<Vertex> {
        self.dag.first_ancestor_nth(name, n)
    }
    fn heads(&self, set: Set) -> dag::Result<Set> {
        self.dag.heads(set)
    }
    fn children(&self, set: Set) -> dag::Result<Set> {
        self.dag.children(set)
    }
    fn roots(&self, set: Set) -> dag::Result<Set> {
        self.dag.roots(set)
    }
    fn gca_one(&self, set: Set) -> dag::Result<Option<Vertex>> {
        self.dag.gca_one(set)
    }
    fn gca_all(&self, set: Set) -> dag::Result<Set> {
        self.dag.gca_all(set)
    }
    fn common_ancestors(&self, set: Set) -> dag::Result<Set> {
        self.dag.common_ancestors(set)
    }
    fn is_ancestor(&self, ancestor: Vertex, descendant: Vertex) -> dag::Result<bool> {
        self.dag.is_ancestor(ancestor, descendant)
    }
    fn heads_ancestors(&self, set: Set) -> dag::Result<Set> {
        self.dag.heads_ancestors(set)
    }
    fn range(&self, roots: Set, heads: Set) -> dag::Result<Set> {
        self.dag.range(roots, heads)
    }
    fn only(&self, reachable: Set, unreachable: Set) -> dag::Result<Set> {
        self.dag.only(reachable, unreachable)
    }
    fn only_both(&self, reachable: Set, unreachable: Set) -> dag::Result<(Set, Set)> {
        self.dag.only_both(reachable, unreachable)
    }
    fn descendants(&self, set: Set) -> dag::Result<Set> {
        self.dag.descendants(set)
    }
    fn reachable_roots(&self, roots: Set, heads: Set) -> dag::Result<Set> {
        self.dag.reachable_roots(roots, heads)
    }
    fn snapshot_dag(&self) -> dag::Result<Arc<dyn DagAlgorithm + Send + Sync>> {
        self.dag.snapshot_dag()
    }
}

impl ToIdSet for MemHgCommits {
    fn to_id_set(&self, set: &Set) -> dag::Result<IdSet> {
        self.dag.to_id_set(set)
    }
}

impl ToSet for MemHgCommits {
    fn to_set(&self, set: &IdSet) -> dag::Result<Set> {
        self.dag.to_set(set)
    }
}

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
}
