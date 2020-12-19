/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::AbstractNameDag;
use crate::iddag::IdDag;
use crate::iddagstore::InProcessStore;
use crate::idmap::MemIdMap;
use crate::ops::Open;
use crate::ops::Persist;
use crate::Result;
use std::sync::atomic::{self, AtomicU64};

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
pub struct MemNameDagState;

impl Open for MemNameDagPath {
    type OpenTarget = MemNameDag;

    fn open(&self) -> Result<Self::OpenTarget> {
        let dag = IdDag::new_in_process();
        let map = MemIdMap::new();
        Ok(AbstractNameDag {
            dag,
            map,
            path: self.clone(),
            snapshot: Default::default(),
            pending_heads: Default::default(),
            state: MemNameDagState,
            id: format!("mem:{}", next_id()),
        })
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
        Ok(())
    }
}

fn next_id() -> u64 {
    static ID: AtomicU64 = AtomicU64::new(0);
    ID.fetch_add(1, atomic::Ordering::AcqRel)
}
