/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! pyedenapi - Python bindings for the Rust `edenapi` crate

#![allow(non_camel_case_types)]

use std::str::{self, FromStr};

use cpython::*;

use cpython_ext::{PyNone, PyPathBuf, ResultPyErrExt};
use edenapi::{Builder, Client, EdenApiBlocking, Progress, ProgressCallback, RepoName, Stats};
use edenapi_types::DataEntry;
use pyrevisionstore::{mutabledeltastore, mutablehistorystore};
use revisionstore::{Delta, HgIdMutableDeltaStore, HgIdMutableHistoryStore, Metadata};
use types::{Key, Node, RepoPathBuf};

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "edenapi"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<client>(py)?;

    Ok(m)
}

fn get_deltastore(py: Python, store: PyObject) -> PyResult<Box<dyn HgIdMutableDeltaStore + Send>> {
    if let Ok(store) = store.extract::<mutabledeltastore>(py) {
        Ok(Box::new(store))
    } else {
        Err(PyErr::new::<exc::RuntimeError, _>(
            py,
            format!("Unknown store {}", store),
        ))
    }
}

fn get_historystore(
    py: Python,
    store: PyObject,
) -> PyResult<Box<dyn HgIdMutableHistoryStore + Send>> {
    if let Ok(store) = store.extract::<mutablehistorystore>(py) {
        Ok(Box::new(store))
    } else {
        Err(PyErr::new::<exc::RuntimeError, _>(
            py,
            format!("Unknown store {}", store),
        ))
    }
}

fn add_data(
    py: Python,
    store: &dyn HgIdMutableDeltaStore,
    entries: impl IntoIterator<Item = DataEntry>,
) -> PyResult<()> {
    for entry in entries {
        let key = entry.key().clone();
        let data = entry.data().map_pyerr(py)?;
        let metadata = Metadata {
            size: Some(data.len() as u64),
            flags: None,
        };
        let delta = Delta {
            data,
            base: None,
            key,
        };
        store.add(&delta, &metadata).map_pyerr(py)?;
    }
    Ok(())
}

py_class!(class client |py| {
    data repo: RepoName;
    data inner: Client;

    def __new__(
        _cls,
        url: String,
        repo: String,
        creds: Option<(PyPathBuf, PyPathBuf)> = None,
        maxfiles: Option<usize> = None,
        maxhistory: Option<usize> = None,
        maxtrees: Option<usize> = None
    ) -> PyResult<client> {
        let url = url.parse().map_pyerr(py)?;
        let repo = repo.parse().map_pyerr(py)?;

        let mut builder = Builder::new()
            .server_url(url)
            .max_files(maxfiles)
            .max_history(maxhistory)
            .max_trees(maxtrees);

        if let Some((cert, key)) = creds {
            builder = builder.client_creds(cert.as_path(), key.as_path());
        }

        let inner = builder.build().map_pyerr(py)?;
        client::create_instance(py, repo, inner)
    }

    def health_check(&self) -> PyResult<PyNone> {
        Ok(self.inner(py).health_blocking().map(|_| PyNone).map_pyerr(py)?)
    }

    def get_files(
        &self,
        keys: Vec<(PyPathBuf, String)>,
        store: PyObject,
        progress_fn: Option<PyObject> = None
    ) -> PyResult<stats> {
        let keys = keys.into_iter()
            .map(|(path, node)| make_key(py, &path, &node))
            .collect::<PyResult<Vec<Key>>>()?;

        let store = get_deltastore(py, store)?;

        let client = self.inner(py);
        let repo = self.repo(py).clone();
        let progress_fn = progress_fn.map(wrap_callback);
        let response = py.allow_threads(move || {
            client.files_blocking(repo, keys, progress_fn)
        }).map_pyerr(py)?;

        add_data(py, &store, response.entries)?;

        stats::create_instance(py, response.stats)
    }

    def get_history(
        &self,
        keys: Vec<(PyPathBuf, String)>,
        store: PyObject,
        depth: Option<u32> = None,
        progress_fn: Option<PyObject> = None
    ) -> PyResult<stats> {
        let keys = keys.into_iter()
            .map(|(path, node)| make_key(py, &path, &node))
            .collect::<PyResult<Vec<Key>>>()?;

        let store = get_historystore(py, store)?;

        let client = self.inner(py);
        let repo = self.repo(py).clone();
        let progress_fn = progress_fn.map(wrap_callback);
        let response = py.allow_threads(move || {
            client.history_blocking(repo, keys, depth, progress_fn)
        }).map_pyerr(py)?;

        for entry in response.entries {
            store.add_entry(&entry).map_pyerr(py)?;
        }

        stats::create_instance(py, response.stats)
    }

    def get_trees(
        &self,
        keys: Vec<(PyPathBuf, String)>,
        store: PyObject,
        progress_fn: Option<PyObject> = None
    ) -> PyResult<stats> {
        let keys = keys.into_iter()
            .map(|(path, node)| make_key(py, &path, &node))
            .collect::<PyResult<Vec<Key>>>()?;

        let store = get_deltastore(py, store)?;

        let client = self.inner(py);
        let repo = self.repo(py).clone();
        let progress_fn = progress_fn.map(wrap_callback);
        let response = py.allow_threads(move || {
            client.trees_blocking(repo, keys, progress_fn)
        }).map_pyerr(py)?;

        add_data(py, &store, response.entries)?;

        stats::create_instance(py, response.stats)
    }

    def prefetch_trees(
        &self,
        rootdir: PyPathBuf,
        mfnodes: Vec<PyBytes>,
        basemfnodes: Vec<PyBytes>,
        store: PyObject,
        depth: Option<usize> = None,
        progress_fn: Option<PyObject> = None
    )  -> PyResult<stats> {
        let rootdir = make_path(py, &rootdir)?;
        let mfnodes = mfnodes
            .into_iter()
            .map(|node| make_node_from_bytes(py, &node))
            .collect::<PyResult<Vec<_>>>()?;
        let basemfnodes = basemfnodes
            .into_iter()
            .map(|node| make_node_from_bytes(py, &node))
            .collect::<PyResult<Vec<_>>>()?;

        let store = get_deltastore(py, store)?;

        let client = self.inner(py);
        let repo = self.repo(py).clone();
        let progress_fn = progress_fn.map(wrap_callback);
        let response = py.allow_threads(move || {
            client.complete_trees_blocking(repo, rootdir, mfnodes, basemfnodes, depth, progress_fn)
        }).map_pyerr(py)?;

        add_data(py, &store, response.entries)?;

        stats::create_instance(py, response.stats)
    }
});

py_class!(class stats |py| {
    data stats: Stats;

    def downloaded(&self) -> PyResult<usize> {
        Ok(self.stats(py).downloaded)
    }

    def uploaded(&self) -> PyResult<usize> {
        Ok(self.stats(py).uploaded)
    }

    def requests(&self) -> PyResult<usize> {
        Ok(self.stats(py).requests)
    }

    def time_in_seconds(&self) -> PyResult<f64> {
        Ok(self.stats(py).time_in_seconds())
    }

    def time_in_millis(&self) -> PyResult<usize> {
        Ok(self.stats(py).time.as_millis() as usize)
    }

    def latency_in_millis(&self) -> PyResult<usize> {
        Ok(self.stats(py).latency.as_millis() as usize)
    }

    def bytes_per_second(&self) -> PyResult<f64> {
        Ok(self.stats(py).bytes_per_second())
    }

    def to_str(&self) -> PyResult<String> {
        Ok(format!("{}", &self.stats(py)))
    }
});

fn make_key(py: Python, path: &PyPathBuf, node: &String) -> PyResult<Key> {
    let path = make_path(py, path)?;
    let node = make_node_from_utf8(py, node)?;
    Ok(Key::new(path, node))
}

fn make_node_from_utf8(py: Python, node: &String) -> PyResult<Node> {
    Ok(Node::from_str(node).map_pyerr(py)?)
}

fn make_node_from_bytes(py: Python, node: &PyBytes) -> PyResult<Node> {
    Ok(Node::from_slice(node.data(py)).map_pyerr(py)?)
}

fn make_path(py: Python, path: &PyPathBuf) -> PyResult<RepoPathBuf> {
    path.to_repo_path()
        .map_pyerr(py)
        .map(|path| path.to_owned())
}

fn wrap_callback(callback: PyObject) -> ProgressCallback {
    Box::new(move |progress: Progress| {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let _ = callback.call(py, progress.as_tuple(), None);
    })
}
