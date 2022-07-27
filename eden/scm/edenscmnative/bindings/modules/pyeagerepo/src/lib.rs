/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use async_runtime::block_on;
use cpython::*;
use cpython_ext::convert::Serde;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::ResultPyErrExt;
use dag::ops::DagAlgorithm;
use eagerepo::EagerRepo as RustEagerRepo;
use eagerepo::EagerRepoStore as RustEagerRepoStore;
use edenapi_types::HgId;
use pydag::dagalgo::dagalgo as PyDag;
use pyedenapi::PyClient;

mod impl_into;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "eagerepo"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<EagerRepo>(py)?;
    m.add_class::<EagerRepoStore>(py)?;
    impl_into::register(py);
    Ok(m)
}

py_class!(class EagerRepo |py| {
    data path: PathBuf;
    data inner: RefCell<RustEagerRepo>;

    /// Construct `EagerRepo` from a directory.
    @staticmethod
    def open(dir: &PyPath) -> PyResult<Self> {
        let path = dir.as_path().to_path_buf();
        let inner = RustEagerRepo::open(&path).map_pyerr(py)?;
        Self::create_instance(py, path, RefCell::new(inner))
    }

    /// Construct `EagerRepo` from a URL.
    @staticmethod
    def openurl(url: &str) -> PyResult<Self> {
        let dir = match RustEagerRepo::url_to_dir(url) {
            Some(dir) => dir,
            None => return Err(PyErr::new::<exc::ValueError, _>(py, "invalid url")),
        };
        let inner = RustEagerRepo::open(&dir).map_pyerr(py)?;
        Self::create_instance(py, dir, RefCell::new(inner))
    }

    /// Write pending changes to disk.
    def flush(&self) -> PyResult<PyNone> {
        let mut inner = self.inner(py).borrow_mut();
        block_on(inner.flush()).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Insert SHA1 blob to zstore.
    /// In hg's case, the `data` is `min(p1, p2) + max(p1, p2) + text`.
    /// (rawtext: bytes) -> node
    def addsha1blob(&self, data: PyBytes) -> PyResult<PyBytes> {
        let mut inner = self.inner(py).borrow_mut();
        let id = inner.add_sha1_blob(data.data(py)).map_pyerr(py)?;
        Ok(PyBytes::new(py, id.as_ref()))
    }

    /// Insert SHA1 blob to zstore.
    /// In hg's case, the `data` is `min(p1, p2) + max(p1, p2) + text`.
    /// (node) -> rawtext
    def getsha1blob(&self, node: PyBytes) -> PyResult<Option<PyBytes>> {
        let inner = self.inner(py).borrow();
        let id = HgId::from_slice(node.data(py)).map_pyerr(py)?;
        let data = inner.get_sha1_blob(id).map_pyerr(py)?;
        Ok(data.map(|d| PyBytes::new(py, d.as_ref())))
    }

    /// Insert a commit. Return the commit hash.
    /// (parents: [node], rawtext: bytes) -> node
    def addcommit(&self, parents: Vec<PyBytes>, rawtext: PyBytes) -> PyResult<PyBytes> {
        let parents: Vec<HgId> = parents.into_iter()
            .map(|p| HgId::from_slice(p.data(py))).collect::<Result<_, _>>().map_pyerr(py)?;
        let id = block_on(self.inner(py).borrow_mut().add_commit(&parents, rawtext.data(py))).map_pyerr(py)?;
        Ok(PyBytes::new(py, id.as_ref()))
    }

    /// Add or remove a bookmark. (name: str, node | None)
    def setbookmark(&self, name: String, node: Option<PyBytes>) -> PyResult<PyNone> {
        let id = match node {
            Some(node) => Some(HgId::from_slice(node.data(py)).map_pyerr(py)?),
            None => None,
        };
        self.inner(py).borrow_mut().set_bookmark(&name, id).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Obtain an edenapi client.
    /// It re-opens the on-disk state so pending changes won't be exposed.
    def edenapiclient(&self) -> PyResult<PyClient> {
        let inner = RustEagerRepo::open(&self.path(py)).map_pyerr(py)?;
        PyClient::from_edenapi(py, Arc::new(inner))
    }

    /// Obtain a dag snapshot.
    def dag(&self) -> PyResult<PyDag> {
        let inner = self.inner(py).borrow();
        let dag = inner.dag().dag_snapshot().map_pyerr(py)?;
        PyDag::from_arc_dag(py, dag)
    }

    /// Obtain a store object.
    def store(&self) -> PyResult<EagerRepoStore> {
        let store = self.inner(py).borrow().store();
        EagerRepoStore::create_instance(py, store)
    }
});

py_class!(pub(crate) class EagerRepoStore |py| {
    data inner: RustEagerRepoStore;

    /// Construct `EagerRepoStore` from a directory.
    @staticmethod
    def open(dir: &PyPath) -> PyResult<Self> {
        let path = dir.as_path().to_path_buf();
        let inner = RustEagerRepoStore::open(&path).map_pyerr(py)?;
        Self::create_instance(py, inner)
    }

    def flush(&self) -> PyResult<PyNone> {
        self.inner(py).flush().map_pyerr(py)?;
        Ok(PyNone)
    }

    /// (data, bases=[]) -> bytes
    ///
    /// `data` should match hg's SHA1 format: min(p1, p2) + max(p1, p2) + raw_text.
    /// For file content with renames, `raw_text` should include the rename filelog header.
    /// Returns sha1(data).
    ///
    /// Changes are buffered in memory until flush().
    def add_sha1_blob(&self, data: PyBytes, bases: Option<Serde<Vec<HgId>>> = None) -> PyResult<PyBytes> {
        let inner = self.inner(py);
        let bases = match bases {
            Some(bases) => bases.0,
            None => Vec::new(),
        };
        let id = inner.add_sha1_blob(data.data(py), &bases).map_pyerr(py)?;
        Ok(PyBytes::new(py, id.as_ref()))
    }

    /// (node) -> Optional[bytes].
    ///
    /// Get the raw text with p1, p2 prefix.
    def get_sha1_blob(&self, node: Serde<HgId>) -> PyResult<Option<PyBytes>> {
        let inner = self.inner(py);
        inner.get_sha1_blob(node.0).map_pyerr(py).map(|d| d.map(|d| PyBytes::new(py, d.as_ref())))
    }

    /// (node) -> Optional[bytes].
    ///
    /// Get the raw text without the p1, p2 prefix.
    /// The raw text includes filelog header for file content.
    def get_content(&self, node: Serde<HgId>) -> PyResult<Option<PyBytes>> {
        let inner = self.inner(py);
        inner.get_content(node.0).map_pyerr(py).map(|d| d.map(|d| PyBytes::new(py, d.as_ref())))
    }
});
