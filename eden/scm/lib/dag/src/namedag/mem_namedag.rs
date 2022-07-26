/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::atomic;
use std::sync::atomic::AtomicU64;

use super::AbstractNameDag;
use super::NameDagBuilder;
use crate::iddag::IdDag;
use crate::iddagstore::InProcessStore;
use crate::idmap::MemIdMap;
use crate::ops::IntVersion;
use crate::ops::Open;
use crate::ops::Persist;
use crate::Result;

/// In-memory version of [`NameDag`].
///
/// Does not support loading from or saving to the filesystem.
/// The graph has to be built from scratch by `add_heads`.
pub type MemNameDag =
    AbstractNameDag<IdDag<InProcessStore>, MemIdMap, MemNameDagPath, MemNameDagState>;

/// Address to open in-memory Dag.
#[derive(Debug, Clone)]
pub struct MemNameDagPath;

#[derive(Debug, Clone)]
pub struct MemNameDagState {
    version: (u64, u64),
}

impl Default for MemNameDagState {
    fn default() -> Self {
        Self {
            version: (rand::random(), 0),
        }
    }
}

impl Open for MemNameDagPath {
    type OpenTarget = MemNameDag;

    fn open(&self) -> Result<Self::OpenTarget> {
        let dag = IdDag::new_in_process();
        let map = MemIdMap::new();
        let id = format!("mem:{}", next_id());
        let state = MemNameDagState::default();
        let result = NameDagBuilder::new_with_idmap_dag(map, dag)
            .with_path(self.clone())
            .with_id(id)
            .with_state(state)
            .build()?;
        Ok(result)
    }
}

impl MemNameDag {
    pub fn new() -> Self {
        MemNameDagPath.open().unwrap()
    }
}

impl Persist for MemNameDagState {
    type Lock = ();

    fn lock(&mut self) -> Result<Self::Lock> {
        Ok(())
    }

    fn reload(&mut self, _lock: &Self::Lock) -> Result<()> {
        Ok(())
    }

    fn persist(&mut self, _lock: &Self::Lock) -> Result<()> {
        self.version.1 += 1;
        Ok(())
    }
}

impl IntVersion for MemNameDagState {
    fn int_version(&self) -> (u64, u64) {
        self.version
    }
}

fn next_id() -> u64 {
    static ID: AtomicU64 = AtomicU64::new(0);
    ID.fetch_add(1, atomic::Ordering::AcqRel)
}
