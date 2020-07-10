/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cpython::*;

use cpython_ext::{ExtractInner, PyNone, PyPathBuf, ResultPyErrExt};
use edenapi::{Builder, EdenApi};
use pyconfigparser::config;

use crate::pyext::EdenApiPyExt;
use crate::stats::stats;

// Python wrapper around an EdenAPI client.
//
// This is basically just FFI boilerplate. The actual functionality
// is implemented as the default implementations of the methods in
// the `EdenApiPyExt` trait.
py_class!(pub class client |py| {
    data inner: Arc<dyn EdenApi>;

    def __new__(_cls, config: config) -> PyResult<client> {
        let config = config.get_cfg(py);
        let inner = Builder::from_config(&config)
            .map_pyerr(py)?
            .build()
            .map_pyerr(py)?;
        client::create_instance(py, Arc::new(inner))
    }

    def health(&self) -> PyResult<PyNone> {
        self.inner(py).clone().health_py(py)
    }

    def files(
        &self,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        store: PyObject,
        progress: Option<PyObject> = None
    ) -> PyResult<stats> {
        self.inner(py).clone().files_py(py, repo, keys, store, progress)
    }

    def history(
        &self,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        store: PyObject,
        length: Option<u32> = None,
        progress: Option<PyObject> = None
    ) -> PyResult<stats> {
        self.inner(py).clone().history_py(py, repo, keys, store, length, progress)
    }

    def trees(
        &self,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        store: PyObject,
        progress: Option<PyObject> = None
    ) -> PyResult<stats> {
        self.inner(py).clone().trees_py(py, repo, keys, store, progress)
    }

    def complete_trees(
        &self,
        repo: String,
        rootdir: PyPathBuf,
        mfnodes: Vec<PyBytes>,
        basemfnodes: Vec<PyBytes>,
        store: PyObject,
        depth: Option<usize> = None,
        progress: Option<PyObject> = None
    )  -> PyResult<stats> {
        self.inner(py).clone().complete_trees_py(
            py,
            repo,
            rootdir,
            mfnodes,
            basemfnodes,
            store,
            depth,
            progress,
        )
    }
});

impl ExtractInner for client {
    type Inner = Arc<dyn EdenApi>;

    fn extract_inner(&self, py: Python) -> Self::Inner {
        self.inner(py).clone()
    }
}
