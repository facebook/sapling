// Copyright 2018 Facebook, Inc.
//! pyedenapi - Python bindings for the Rust `edenapi` crate

#![allow(non_camel_case_types)]

use std::str;

use cpython::*;

use cpython_failure::ResultPyErrExt;
use edenapi::{Config, DownloadStats, EdenApi, EdenApiCurlClient, ProgressFn, ProgressStats};
use encoding::local_bytes_to_path;
use types::{Key, Node, RepoPath};

use crate::revisionstore::{PythonMutableDataPack, PythonMutableHistoryPack};

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "edenapi"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<client>(py)?;
    Ok(m)
}

py_class!(class client |py| {
    data inner: EdenApiCurlClient;

    def __new__(
        _cls,
        url: &PyBytes,
        cachepath: &PyBytes,
        repo: &PyBytes,
        creds: Option<(&PyBytes, &PyBytes)> = None,
        databatchsize: Option<usize> = None,
        historybatchsize: Option<usize> = None,
        validate: bool = true
    ) -> PyResult<client> {
        let url = str::from_utf8(url.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
        let cachepath = local_bytes_to_path(&cachepath.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
        let repo = str::from_utf8(repo.data(py)).map_pyerr::<exc::RuntimeError>(py)?;

        let mut config = Config::new()
            .base_url_str(url)
            .map_pyerr::<exc::RuntimeError>(py)?
            .cache_path(cachepath)
            .repo(repo)
            .data_batch_size(databatchsize)
            .history_batch_size(historybatchsize)
            .validate(validate);

        if let Some((cert, key)) = creds {
            let cert = local_bytes_to_path(cert.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
            let key = local_bytes_to_path(key.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
            config = config.client_creds(cert, key);
        }

        let inner = EdenApiCurlClient::new(config).map_pyerr::<exc::RuntimeError>(py)?;
        client::create_instance(py, inner)
    }

    def health_check(&self) -> PyResult<PyObject> {
        self.inner(py).health_check()
            .map(|()| py.None())
            .map_pyerr::<exc::RuntimeError>(py)
    }

    def hostname(&self) -> PyResult<String> {
        self.inner(py).hostname().map_pyerr::<exc::RuntimeError>(py)
    }

    def get_files(&self, keys: Vec<(PyBytes, PyBytes)>, store: PyObject, progress_fn: Option<PyObject> = None) -> PyResult<downloadstats> {
        let keys = keys.into_iter()
            .map(|(path, node)| make_key(py, &path, &node))
            .collect::<PyResult<Vec<Key>>>()?;

        let mut store = PythonMutableDataPack::new(store)?;

        let client = self.inner(py);
        let progress_fn = progress_fn.map(wrap_callback);
        let stats = py.allow_threads(move || {
            client.get_files(keys, &mut store, progress_fn)
        }).map_pyerr::<exc::RuntimeError>(py)?;

        downloadstats::create_instance(py, stats)
    }

    def get_history(
        &self,
        keys: Vec<(PyBytes, PyBytes)>,
        store: PyObject,
        depth: Option<u32> = None,
        progress_cb: Option<PyObject> = None
    ) -> PyResult<downloadstats> {
        let keys = keys.into_iter()
            .map(|(path, node)| make_key(py, &path, &node))
            .collect::<PyResult<Vec<Key>>>()?;

        let mut store = PythonMutableHistoryPack::new(store)?;

        let client = self.inner(py);
        let progress_cb = progress_cb.map(wrap_callback);
        let stats = py.allow_threads(move || {
            client.get_history(keys, &mut store, depth, progress_cb)
        }).map_pyerr::<exc::RuntimeError>(py)?;

        downloadstats::create_instance(py, stats)
    }

    def get_trees(&self, keys: Vec<(PyBytes, PyBytes)>, store: PyObject, progress_fn: Option<PyObject> = None) -> PyResult<downloadstats> {
        let keys = keys.into_iter()
            .map(|(path, node)| make_key(py, &path, &node))
            .collect::<PyResult<Vec<Key>>>()?;

        let mut store = PythonMutableDataPack::new(store)?;

        let client = self.inner(py);
        let progress_fn = progress_fn.map(wrap_callback);
        let stats = py.allow_threads(move || {
            client.get_trees(keys, &mut store, progress_fn)
        }).map_pyerr::<exc::RuntimeError>(py)?;

        downloadstats::create_instance(py, stats)
    }
});

py_class!(class downloadstats |py| {
    data stats: DownloadStats;

    def downloaded(&self) -> PyResult<usize> {
        Ok(self.stats(py).downloaded)
    }

    def uploaded(&self) -> PyResult<usize> {
        Ok(self.stats(py).uploaded)
    }

    def requests(&self) -> PyResult<usize> {
        Ok(self.stats(py).requests)
    }

    def time_millis(&self) -> PyResult<usize> {
        Ok(self.stats(py).time.as_millis() as usize)
    }
});

fn make_key(py: Python, path: &PyBytes, node: &PyBytes) -> PyResult<Key> {
    let path = path.data(py);
    let path = RepoPath::from_utf8(path)
        .map_pyerr::<exc::RuntimeError>(py)?
        .to_owned();
    let node = str::from_utf8(node.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
    let node = Node::from_str(node).map_pyerr::<exc::RuntimeError>(py)?;
    Ok(Key::new(path, node))
}

fn wrap_callback(callback: PyObject) -> ProgressFn {
    Box::new(move |stats: ProgressStats| {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let _ = callback.call(py, stats.as_tuple(), None);
    })
}
