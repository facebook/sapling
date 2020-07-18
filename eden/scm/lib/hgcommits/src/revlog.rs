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
use revlogindex::RevlogIndex;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

/// HG commits stored on disk using the revlog format.
pub struct RevlogCommits {
    revlog: RevlogIndex,
    dir: PathBuf,
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

impl IdConvert for RevlogCommits {
    fn vertex_id(&self, name: Vertex) -> Result<Id> {
        self.revlog.vertex_id(name)
    }
    fn vertex_id_with_max_group(&self, name: &Vertex, max_group: Group) -> Result<Option<Id>> {
        self.revlog.vertex_id_with_max_group(name, max_group)
    }
    fn vertex_name(&self, id: Id) -> Result<Vertex> {
        self.revlog.vertex_name(id)
    }
    fn contains_vertex_name(&self, name: &Vertex) -> Result<bool> {
        self.revlog.contains_vertex_name(name)
    }
}

impl PrefixLookup for RevlogCommits {
    fn vertexes_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> Result<Vec<Vertex>> {
        self.revlog.vertexes_by_hex_prefix(hex_prefix, limit)
    }
}

impl DagAlgorithm for RevlogCommits {
    fn sort(&self, set: &Set) -> Result<Set> {
        self.revlog.sort(set)
    }
    fn parent_names(&self, name: Vertex) -> Result<Vec<Vertex>> {
        self.revlog.parent_names(name)
    }
    fn all(&self) -> Result<Set> {
        self.revlog.all()
    }
    fn ancestors(&self, set: Set) -> Result<Set> {
        self.revlog.ancestors(set)
    }
    fn parents(&self, set: Set) -> Result<Set> {
        self.revlog.parents(set)
    }
    fn first_ancestor_nth(&self, name: Vertex, n: u64) -> Result<Vertex> {
        self.revlog.first_ancestor_nth(name, n)
    }
    fn heads(&self, set: Set) -> Result<Set> {
        self.revlog.heads(set)
    }
    fn children(&self, set: Set) -> Result<Set> {
        self.revlog.children(set)
    }
    fn roots(&self, set: Set) -> Result<Set> {
        self.revlog.roots(set)
    }
    fn gca_one(&self, set: Set) -> Result<Option<Vertex>> {
        self.revlog.gca_one(set)
    }
    fn gca_all(&self, set: Set) -> Result<Set> {
        self.revlog.gca_all(set)
    }
    fn common_ancestors(&self, set: Set) -> Result<Set> {
        self.revlog.common_ancestors(set)
    }
    fn is_ancestor(&self, ancestor: Vertex, descendant: Vertex) -> Result<bool> {
        self.revlog.is_ancestor(ancestor, descendant)
    }
    fn heads_ancestors(&self, set: Set) -> Result<Set> {
        self.revlog.heads_ancestors(set)
    }
    fn range(&self, roots: Set, heads: Set) -> Result<Set> {
        self.revlog.range(roots, heads)
    }
    fn only(&self, reachable: Set, unreachable: Set) -> Result<Set> {
        self.revlog.only(reachable, unreachable)
    }
    fn only_both(&self, reachable: Set, unreachable: Set) -> Result<(Set, Set)> {
        self.revlog.only_both(reachable, unreachable)
    }
    fn descendants(&self, set: Set) -> Result<Set> {
        self.revlog.descendants(set)
    }
}

impl ToIdSet for RevlogCommits {
    fn to_id_set(&self, set: &Set) -> Result<IdSet> {
        self.revlog.to_id_set(set)
    }
}

impl ToSet for RevlogCommits {
    fn to_set(&self, set: &IdSet) -> Result<Set> {
        self.revlog.to_set(set)
    }
}
