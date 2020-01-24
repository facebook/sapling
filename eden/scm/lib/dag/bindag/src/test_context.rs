/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{parse_bindag, ParentRevs};
use anyhow::Result;
use dag::{namedag::LowLevelAccess, spanset::SpanSet, Id, IdDag, NameDag, VertexName};
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

    /// Simple IdMap. NameDag id -> Plain DAG id
    pub idmap: Vec<usize>,

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

impl<T: AsRef<[usize]>> GeneralTestContext<T> {
    pub fn from_parents(parents: Vec<T>) -> Self {
        // Prepare NameDag
        let parents_by_name = |name: VertexName| -> Result<Vec<VertexName>> {
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
        let mut heads: HashSet<usize> = (0..parents.len()).collect();
        for ps in &parents {
            for p in ps.as_ref().iter() {
                heads.remove(&p);
            }
        }
        let mut head_names: Vec<VertexName> = Vec::new();
        for h in heads {
            head_names.push(VertexName::copy_from(format!("{}", h).as_bytes()));
        }

        let dir = tempfile::tempdir().unwrap();
        let mut dag = NameDag::open(dir.path()).unwrap();
        dag.add_heads_and_flush(&parents_by_name, &head_names, &[])
            .unwrap();

        // Prepare idmap
        let idmap = (0..parents.len())
            .map(|i| {
                std::str::from_utf8(dag.map().find_name_by_id(Id(i as u64)).unwrap().unwrap())
                    .unwrap()
                    .parse()
                    .unwrap()
            })
            .collect();

        Self {
            parents,
            dag,
            idmap,
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

    /// Get the IdDag reference.
    pub fn id_dag(&self) -> &IdDag {
        self.dag.dag()
    }

    /// Convert a SpanSet (used by IdDag) to plain revs (used by `parents`).
    pub fn to_plain_revs(&self, set: &SpanSet) -> Vec<usize> {
        set.iter().map(|i| self.idmap[i.0 as usize]).collect()
    }

    /// Make `revs` in the range of parents.
    pub fn clamp_revs(&self, revs: &[impl Into<usize> + Clone]) -> Vec<usize> {
        revs.iter()
            .cloned()
            .map(|i| Into::<usize>::into(i) % self.parents.len())
            .collect()
    }
}
