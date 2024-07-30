/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

use indexedlog::multi;
use indexedlog::DefaultOpenOptions;
use indexedlog::OpenWithRepair;

use super::AbstractDag;
use super::DagBuilder;
use crate::errors::bug;
use crate::iddag::IdDag;
use crate::iddagstore::IndexedLogStore;
use crate::idmap::IdMap;
use crate::ops::Open;
use crate::ops::Persist;
use crate::ops::StorageVersion;
use crate::ops::TryClone;
use crate::Result;

/// A DAG that uses Vertex instead of ids as vertexes.
///
/// A high-level wrapper structure. Combination of [`IdMap`] and [`Dag`].
/// Maintains consistency of dag and map internally.
pub type Dag = AbstractDag<IdDag<IndexedLogStore>, IdMap, IndexedLogDagPath, DagState>;

pub struct DagState {
    /// `MultiLog` controls on-disk metadata.
    /// `None` for read-only `Dag`,
    mlog: Option<multi::MultiLog>,
}

/// Address to on-disk Dag based on indexedlog.
#[derive(Debug, Clone)]
pub struct IndexedLogDagPath(pub PathBuf);

impl Open for IndexedLogDagPath {
    type OpenTarget = Dag;

    fn open(&self) -> Result<Self::OpenTarget> {
        crate::failpoint!("dag-dag-open");
        let path = &self.0;
        let opts = Dag::default_open_options();
        tracing::debug!(target: "dag::open",  "open at {:?}", path.display());
        let mut mlog = opts.open_with_repair(path)?;
        let mut logs = mlog.detach_logs();
        let dag_log = logs.pop().unwrap();
        let map_log = logs.pop().unwrap();
        let map = IdMap::open_from_log(map_log)?;
        let dag = IdDag::open_from_store(IndexedLogStore::open_from_clean_log(dag_log)?)?;
        let state = DagState { mlog: Some(mlog) };
        let id = format!("ilog:{}", self.0.display());
        let dag = DagBuilder::new_with_idmap_dag(map, dag)
            .with_path(self.clone())
            .with_state(state)
            .with_id(id)
            .build()?;
        Ok(dag)
    }
}

impl DefaultOpenOptions<multi::OpenOptions> for Dag {
    fn default_open_options() -> multi::OpenOptions {
        multi::OpenOptions::from_name_opts(vec![
            ("idmap2", IdMap::log_open_options()),
            ("iddag", IndexedLogStore::log_open_options()),
        ])
    }
}

impl Dag {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let path = IndexedLogDagPath(path);
        path.open()
    }
}

impl Persist for DagState {
    type Lock = indexedlog::multi::LockGuard;

    fn lock(&mut self) -> Result<Self::Lock> {
        if self.mlog.is_none() {
            return bug("MultiLog should be Some for read-write Dag");
        }
        let mlog = self.mlog.as_mut().unwrap();
        // mlog.lock() reloads its MultiMeta, but not Logs.
        //
        // Usually the use pattern is like:
        //
        //    let locked = self.state.prepare_filesystem_sync()?;  // Get the latest MultiMeta
        //    let mut map = self.map.prepare_filesystem_sync()?;   // Get the latest Log
        //    let mut dag = self.dag.prepare_filesystem_sync()?;   // Get the latest Log.
        //
        // The `DagState` does not control the `map` or `dag` Logs so it cannot reload
        // them here, or in `reload()`.
        Ok(mlog.lock()?)
    }

    fn reload(&mut self, _lock: &Self::Lock) -> Result<()> {
        // mlog does reload internally. See `lock()`.
        Ok(())
    }

    fn persist(&mut self, lock: &Self::Lock) -> Result<()> {
        self.mlog.as_mut().unwrap().write_meta(lock)?;
        Ok(())
    }
}

impl StorageVersion for DagState {
    fn storage_version(&self) -> (u64, u64) {
        match &self.mlog {
            Some(mlog) => mlog.version(),
            None => (0, 0),
        }
    }
}

impl TryClone for DagState {
    fn try_clone(&self) -> Result<Self> {
        Ok(Self {
            // mlog cannot be cloned.
            mlog: None,
        })
    }
}
