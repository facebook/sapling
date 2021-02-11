/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cpython::*;

use cpython_async::PyFuture;
use cpython_async::TStream;
use cpython_ext::convert::Serde;
use cpython_ext::{ExtractInner, ExtractInnerRef, PyPathBuf, ResultPyErrExt};
use edenapi::{Builder, EdenApi};
use edenapi_types::{CommitLocationToHashResponse, CommitRevlogData};
use progress::{NullProgressFactory, ProgressFactory};
use pyconfigparser::config;
use pyprogress::PyProgressFactory;
use pyrevisionstore::{edenapifilestore, edenapitreestore};
use revisionstore::{EdenApiFileStore, EdenApiTreeStore};

use crate::pyext::EdenApiPyExt;
use crate::stats::stats;

// Python wrapper around an EdenAPI client.
//
// This is basically just FFI boilerplate. The actual functionality
// is implemented as the default implementations of the methods in
// the `EdenApiPyExt` trait.
py_class!(pub class client |py| {
    data inner: Arc<dyn EdenApi>;
    data progress: Arc<dyn ProgressFactory>;

    def __new__(
        _cls,
        config: config,
        ui: Option<PyObject> = None,
        correlator: Option<String> = None
    ) -> PyResult<client> {
        let config = config.get_cfg(py);
        let inner = Builder::from_config(&config)
            .map_pyerr(py)?
            .correlator(correlator)
            .header("User-Agent", format!("EdenSCM/{}", version::VERSION))
            .build()
            .map_pyerr(py)?;

        let progress = match ui {
            Some(ui) => PyProgressFactory::arc(py, ui)?,
            None => NullProgressFactory::arc(),
        };

        client::create_instance(py, Arc::new(inner), progress)
    }

    def health(&self) -> PyResult<PyDict> {
        self.inner(py).clone().health_py(py)
    }

    def files(
        &self,
        store: PyObject,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        callback: Option<PyObject> = None
    ) -> PyResult<stats> {
        let progress = self.progress(py).clone();
        self.inner(py).clone().files_py(py, store, repo, keys, callback, progress)
    }

    def history(
        &self,
        store: PyObject,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        length: Option<u32> = None,
        callback: Option<PyObject> = None
    ) -> PyResult<stats> {
        let progress = self.progress(py).clone();
        self.inner(py).clone().history_py(py, store, repo, keys, length, callback, progress)
    }

    def trees(
        &self,
        store: PyObject,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        attributes: Option<PyDict> = None,
        callback: Option<PyObject> = None
    ) -> PyResult<stats> {
        let progress = self.progress(py).clone();
        self.inner(py).clone().trees_py(py, store, repo, keys, attributes, callback, progress)
    }

    def complete_trees(
        &self,
        store: PyObject,
        repo: String,
        rootdir: PyPathBuf,
        mfnodes: Vec<PyBytes>,
        basemfnodes: Vec<PyBytes>,
        depth: Option<usize> = None,
        callback: Option<PyObject> = None
    )  -> PyResult<stats> {
        let progress = self.progress(py).clone();
        self.inner(py).clone().complete_trees_py(
            py,
            store,
            repo,
            rootdir,
            mfnodes,
            basemfnodes,
            depth,
            callback,
            progress,
        )
    }


    /// commitdata(repo: str, nodes: [bytes], progress=None) -> [(node: bytes, data: bytes)], stats
    ///
    /// Fetch commit data in raw HG format (sorted([p1, p2]) + text).
    def commitdata(
        &self,
        repo: String,
        nodes: Vec<PyBytes>,
        callback: Option<PyObject> = None
    ) -> PyResult<(TStream<anyhow::Result<Serde<CommitRevlogData>>>, PyFuture)> {
        self.inner(py).clone().commit_revlog_data_py(py, repo, nodes, callback)
    }

    def filestore(
        &self,
        repo: String
    ) -> PyResult<edenapifilestore> {
        let edenapi = self.extract_inner(py);
        let progress = Some(self.progress(py).clone());
        let store = EdenApiFileStore::new(repo, edenapi, progress);

        edenapifilestore::new(py, store)
    }

    def treestore(
        &self,
        repo: String
    ) -> PyResult<edenapitreestore> {
        let edenapi = self.extract_inner(py);
        let progress = Some(self.progress(py).clone());
        let store = EdenApiTreeStore::new(repo, edenapi, progress);

        edenapitreestore::new(py, store)
    }

    /// commitlocationtohash(repo: str, requests: [(bytes, u64, u64), progress = None] ->
    ///   [(location: (descendant: bytes, distance: u64), count: u64, hgids: [bytes])]
    ///
    /// Fetch the hash(es) of a location in the commit graph.
    /// A request is a tuple (descendant, distance, count) where descendant is a known hgid,
    /// distance represents how many parents we traverse from descendant to the desired commit and
    /// count represents the number of ancestors from the location that we want.
    def commitlocationtohash(
        &self,
        repo: String,
        requests: Vec<(PyBytes, u64, u64)>,
        callback: Option<PyObject> = None
    ) -> PyResult<(TStream<anyhow::Result<Serde<CommitLocationToHashResponse>>>, PyFuture)> {
        self.inner(py).clone().commit_location_to_hash_py(py, repo, requests, callback)
    }
});

impl ExtractInnerRef for client {
    type Inner = Arc<dyn EdenApi>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.inner(py)
    }
}
