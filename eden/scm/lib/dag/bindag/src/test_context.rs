/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{parse_bindag, ParentRevs};
use dag::nameset::SyncNameSetQuery;
use dag::ops::DagAlgorithm;
use dag::ops::IdConvert;
use dag::OnDiskIdDag;
use dag::{ops::DagPersistent, spanset::SpanSet, Id, NameDag, VertexName};
use nonblocking::non_blocking_result;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Range;
use tempfile::TempDir;

/// Context for testing purpose.
/// Contains the parsed bindag and NameDag from the dag crate.
pub struct GeneralTestContext<T> {
    /// Plain DAG parsed from bindag
    pub parents: Vec<T>,

    /// Complex DAG, with ids re-assigned
    pub dag: NameDag,

    /// Simple IdMap. NameDag Id -> Plain DAG id
    pub idmap: HashMap<Id, usize>,

    /// Plain DAG id -> NameDag Id.
    pub rev_idmap: Vec<Id>,

    /// Temporary dir containing the NameDag daga.
    pub dir: TempDir,
}

pub type TestContext = GeneralTestContext<ParentRevs>;
pub type OctopusTestContext = GeneralTestContext<Vec<usize>>;

impl TestContext {
    pub fn from_bin(bin: &[u8]) -> Self {
        Self::from_bin_sliced(bin, 0..usize::max_value())
    }

    pub fn from_bin_sliced(bin: &[u8], range: Range<usize>) -> Self {
        // Prepare the plain DAG (parents)
        let parents = parse_bindag(bin);
        let parents = crate::slice_parents(parents, range);
        Self::from_parents(parents)
    }
}

impl<T: AsRef<[usize]> + Send + Sync> GeneralTestContext<T> {
    pub fn from_parents(parents: Vec<T>) -> Self {
        // Prepare NameDag
        let parents_by_name = |name: VertexName| -> dag::Result<Vec<VertexName>> {
            let i = String::from_utf8(name.as_ref().to_vec())
                .unwrap()
                .parse::<usize>()
                .unwrap();
            Ok(parents[i]
                .as_ref()
                .iter()
                .map(|p| format!("{}", p).as_bytes().to_vec().into())
                .collect())
        };
        // Pick heads from 0..n
        let get_heads = |n: usize| -> Vec<VertexName> {
            let mut heads: HashSet<usize> = (0..n).collect();
            for ps in parents.iter().take(n) {
                for p in ps.as_ref().iter() {
                    heads.remove(&p);
                }
            }
            let mut names: Vec<VertexName> = Vec::new();
            for h in heads {
                names.push(VertexName::copy_from(format!("{}", h).as_bytes()));
            }
            names
        };
        let head_names: Vec<VertexName> = get_heads(parents.len());
        let master_names: Vec<VertexName> = get_heads(parents.len() / 2);

        let dir = tempfile::tempdir().unwrap();
        let mut dag = NameDag::open(dir.path()).unwrap();
        non_blocking_result(dag.add_heads_and_flush(&parents_by_name, &master_names, &head_names))
            .unwrap();

        // Prepare idmap
        let idmap: HashMap<Id, usize> = dag
            .all()
            .unwrap()
            .iter()
            .unwrap()
            .map(|name| {
                let name = name.unwrap();
                let dag_id: Id = dag.vertex_id(name.clone()).unwrap();
                let plain_id: usize = std::str::from_utf8(name.as_ref()).unwrap().parse().unwrap();
                (dag_id, plain_id)
            })
            .collect();
        let mut rev_idmap = vec![Id(0); idmap.len()];
        for (&k, &v) in idmap.iter() {
            rev_idmap[v] = k;
        }
        assert_eq!(rev_idmap.len(), parents.len());

        Self {
            parents,
            dag,
            idmap,
            rev_idmap,
            dir,
        }
    }
}

impl<T> GeneralTestContext<T> {
    /// Limit the size of `parents`.
    pub fn truncate(mut self, size: usize) -> Self {
        if size < self.parents.len() {
            self.parents.truncate(size);
        }
        self
    }

    /// Convert a SpanSet (used by IdDag) to plain revs (used by `parents`).
    pub fn to_plain_revs(&self, set: &SpanSet) -> Vec<usize> {
        set.iter().map(|i| self.idmap[&i]).collect()
    }

    /// Get the IdDag reference.
    pub fn id_dag(&self) -> &OnDiskIdDag {
        self.dag.dag()
    }

    /// Make `revs` in the range of parents.
    pub fn clamp_revs(&self, revs: &[impl Into<usize> + Clone]) -> Vec<usize> {
        revs.iter()
            .cloned()
            .map(|i| Into::<usize>::into(i) % self.parents.len())
            .collect()
    }

    /// Convert `usize` plain revs to SpanSet used by IdDag.
    pub fn to_dag_revs(&self, revs: &[usize]) -> SpanSet {
        let iter = revs.iter().map(|&i| self.rev_idmap[i].clone());
        SpanSet::from_spans(iter)
    }
}
