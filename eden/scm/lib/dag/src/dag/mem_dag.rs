/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::atomic;
use std::sync::atomic::AtomicU64;

use super::AbstractDag;
use super::DagBuilder;
use crate::iddag::IdDag;
use crate::iddagstore::MemStore;
use crate::idmap::MemIdMap;
use crate::ops::Open;
use crate::ops::Persist;
use crate::ops::StorageVersion;
use crate::Result;

/// In-memory version of [`Dag`].
///
/// Does not support loading from or saving to the filesystem.
/// The graph has to be built from scratch by `add_heads`.
pub type MemDag = AbstractDag<IdDag<MemStore>, MemIdMap, MemDagPath, MemDagState>;

/// Address to open in-memory Dag.
#[derive(Debug, Clone)]
pub struct MemDagPath;

#[derive(Debug, Clone)]
pub struct MemDagState {
    version: (u64, u64),
}

impl Default for MemDagState {
    fn default() -> Self {
        Self {
            version: (rand::random(), 0),
        }
    }
}

impl Open for MemDagPath {
    type OpenTarget = MemDag;

    fn open(&self) -> Result<Self::OpenTarget> {
        let dag = IdDag::new_in_memory();
        let map = MemIdMap::new();
        let id = format!("mem:{}", next_id());
        let state = MemDagState::default();
        let result = DagBuilder::new_with_idmap_dag(map, dag)
            .with_path(self.clone())
            .with_id(id)
            .with_state(state)
            .build()?;
        Ok(result)
    }
}

impl MemDag {
    pub fn new() -> Self {
        MemDagPath.open().unwrap()
    }
}

impl Persist for MemDagState {
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

impl StorageVersion for MemDagState {
    fn storage_version(&self) -> (u64, u64) {
        self.version
    }
}

fn next_id() -> u64 {
    static ID: AtomicU64 = AtomicU64::new(0);
    ID.fetch_add(1, atomic::Ordering::AcqRel)
}
