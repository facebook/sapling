/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Support `ImplInto` from cpython-ext.

use std::sync::Arc;

use anyhow::Result;
use blob::Blob;
use cpython::*;
use cpython_ext::ExtractInner;
use cpython_ext::convert::register_into;
use revisionstore::HgIdDataStore;
use revisionstore::RemoteDataStore;
use revisionstore::StoreKey;
use revisionstore::StoreResult;
use revisionstore::trait_impls::ArcFileStore;
use storemodel::FileStore;
use storemodel::KeyStore;
use storemodel::SerializationFormat;
use storemodel::TreeStore;
use types::Key;
use types::RepoPath;

use crate::PythonHgIdDataStore;
use crate::filescmstore;
use crate::treescmstore;

pub(crate) fn register(py: Python) {
    register_into(py, |py, t: treescmstore| t.to_dyn_treestore(py));
    register_into(py, py_to_dyn_treestore);

    register_into(py, |py, f: filescmstore| f.to_read_file_contents(py));
    register_into(py, |py, f: filescmstore| f.to_dyn_key_store(py));
    register_into(py, |py, f: filescmstore| f.to_dyn_file_store(py));
}

impl filescmstore {
    fn to_arc_store(&self, py: Python) -> Arc<ArcFileStore> {
        let store = self.extract_inner(py);
        Arc::new(ArcFileStore(store))
    }

    fn to_read_file_contents(&self, py: Python) -> Arc<dyn FileStore> {
        self.to_arc_store(py)
    }

    fn to_dyn_key_store(&self, py: Python) -> Arc<dyn KeyStore> {
        self.to_arc_store(py)
    }

    fn to_dyn_file_store(&self, py: Python) -> Arc<dyn FileStore> {
        self.to_arc_store(py)
    }
}

impl treescmstore {
    fn to_dyn_treestore(&self, py: Python) -> Arc<dyn TreeStore> {
        match &self.caching_store(py) {
            Some(caching_store) => caching_store.clone(),
            None => self.store(py).clone(),
        }
    }
}

// Legacy support for store in Python.
// Used at least by unioncontentstore.
fn py_to_dyn_treestore(_py: Python, obj: PyObject) -> Arc<dyn TreeStore> {
    Arc::new(ManifestStore::new(PythonHgIdDataStore::new(obj)))
}

#[derive(Clone)]
struct ManifestStore {
    underlying: PythonHgIdDataStore,
}

impl ManifestStore {
    pub fn new(underlying: PythonHgIdDataStore) -> Self {
        ManifestStore { underlying }
    }
}

impl KeyStore for ManifestStore {
    fn get_local_content(
        &self,
        path: &RepoPath,
        node: types::HgId,
    ) -> anyhow::Result<Option<Blob>> {
        if node.is_null() {
            return Ok(Some(Blob::Bytes(Default::default())));
        }
        let key = Key::new(path.to_owned(), node);
        match self.underlying.get(StoreKey::hgid(key))? {
            StoreResult::NotFound(_key) => Ok(None),
            StoreResult::Found(data) => Ok(Some(Blob::Bytes(data.into()))),
        }
    }

    fn prefetch(&self, keys: Vec<Key>) -> Result<()> {
        let keys = keys
            .iter()
            .filter_map(|k| {
                if k.hgid.is_null() {
                    None
                } else {
                    Some(StoreKey::from(k))
                }
            })
            .collect::<Vec<_>>();
        self.underlying.prefetch(&keys).map(|_| ())
    }

    fn clone_key_store(&self) -> Box<dyn KeyStore> {
        Box::new(self.clone())
    }

    fn format(&self) -> SerializationFormat {
        self.underlying.format()
    }
}

impl TreeStore for ManifestStore {
    fn clone_tree_store(&self) -> Box<dyn TreeStore> {
        Box::new(self.clone())
    }
}
