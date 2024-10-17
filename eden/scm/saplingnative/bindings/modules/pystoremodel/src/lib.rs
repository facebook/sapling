/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::convert::Serde;
use cpython_ext::ResultPyErrExt;
use storemodel::FileStore as NativeFileStore;
use storemodel::InsertOpts;
use storemodel::SerializationFormat;
use storemodel::TreeItemFlag;
use storemodel::TreeStore as NativeTreeStore;
use types::Id20;
use types::PathComponentBuf;
use types::RepoPath;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "storemodel"].join(".");

    let m = PyModule::new(py, &name)?;
    m.add_class::<FileStore>(py)?;
    m.add_class::<TreeStore>(py)?;

    Ok(m)
}

py_class!(pub class FileStore |py| {
    data inner: Arc<dyn NativeFileStore>;

    /// insert_file(opts, path: str, data: bytes) -> node
    /// opts: {parents: List[node], hg_flags: int}
    ///
    /// Check `storemodel::FileStore` for details.
    def insert_file(&self, opts: Serde<InsertOpts>, path: &str, data: PyBytes) -> PyResult<Serde<Id20>> {
        let inner = self.inner(py);
        let path = RepoPath::from_str(path).map_pyerr(py)?;
        let data = data.data(py);
        let id = py.allow_threads(|| inner.insert_file(opts.0, path, data)).map_pyerr(py)?;
        Ok(Serde(id))
    }

    def flush(&self) -> PyResult<PyNone> {
        let inner = self.inner(py);
        py.allow_threads(|| inner.flush()).map_pyerr(py)?;
        Ok(PyNone)
    }

    @staticmethod
    def from_store(store: ImplInto<Arc<dyn NativeFileStore>>) -> PyResult<Self> {
        let inner = store.into();
        Self::create_instance(py, inner)
    }
});

py_class!(pub class TreeStore |py| {
    data inner: Arc<dyn NativeTreeStore>;

    /// insert_file(opts, path: str, data: bytes) -> node
    /// opts: {parents: List[node], hg_flags: int}
    ///
    /// Check `storemodel::TreeStore` for details.
    def insert_tree(&self, opts: Serde<InsertOpts>, path: &str, items: Serde<Vec<(PathComponentBuf, Id20, TreeItemFlag)>>) -> PyResult<Serde<Id20>> {
        let inner = self.inner(py);
        let path = RepoPath::from_str(path).map_pyerr(py)?;
        let id = py.allow_threads(|| inner.insert_tree(opts.0, path, items.0)).map_pyerr(py)?;
        Ok(Serde(id))
    }

    def flush(&self) -> PyResult<PyNone> {
        let inner = self.inner(py);
        py.allow_threads(|| inner.flush()).map_pyerr(py)?;
        Ok(PyNone)
    }

    def format(&self) -> PyResult<Serde<SerializationFormat>> {
        let inner = self.inner(py);
        Ok(Serde(inner.format()))
    }

    @staticmethod
    def from_store(store: ImplInto<Arc<dyn NativeTreeStore>>) -> PyResult<Self> {
        let inner = store.into();
        Self::create_instance(py, inner)
    }
});
