/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::strip;
use crate::AppendCommits;
use crate::HgCommit;
use crate::ReadCommitText;
use crate::StripCommits;
use anyhow::ensure;
use anyhow::Result;
use dag::ops::DagAddHeads;
use dag::ops::DagAlgorithm;
use dag::ops::DagPersistent;
use dag::ops::IdConvert;
use dag::ops::PrefixLookup;
use dag::ops::ToIdSet;
use dag::ops::ToSet;
use dag::Dag;
use dag::Group;
use dag::Id;
use dag::IdSet;
use dag::Set;
use dag::Vertex;
use minibytes::Bytes;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use zstore::Id20;
use zstore::Zstore;

/// Commits using the HG SHA1 hash function. Stored on disk.
pub struct HgCommits {
    commits: Zstore,
    commits_path: PathBuf,
    dag: Dag,
    dag_path: PathBuf,
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
            ensure!(
                vertex == commit.vertex,
                "hash mismatch ({:?} != {:?})",
                vertex,
                commit.vertex
            );
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

impl IdConvert for HgCommits {
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

impl PrefixLookup for HgCommits {
    fn vertexes_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> dag::Result<Vec<Vertex>> {
        self.dag.vertexes_by_hex_prefix(hex_prefix, limit)
    }
}

impl DagAlgorithm for HgCommits {
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
}

impl ToIdSet for HgCommits {
    fn to_id_set(&self, set: &Set) -> dag::Result<IdSet> {
        self.dag.to_id_set(set)
    }
}

impl ToSet for HgCommits {
    fn to_set(&self, set: &IdSet) -> dag::Result<Set> {
        self.dag.to_set(set)
    }
}
