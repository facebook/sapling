// Copyright 2018 Facebook, Inc.
//! pyedenapi - Python bindings for the Rust `edenapi` crate

#![allow(non_camel_case_types)]

use std::str;

use cpython::*;

use cpython_failure::ResultPyErrExt;
use edenapi::{
    ApiError, ApiErrorKind, Config, DownloadStats, EdenApi, EdenApiCurlClient, ProgressFn,
    ProgressStats,
};
use encoding::local_bytes_to_path;
use revisionstore::{MutableDeltaStore, MutableHistoryStore};
use types::{Key, Node, RepoPath, RepoPathBuf};

use crate::revisionstore::{
    mutabledeltastore, mutablehistorystore, PythonMutableDataPack, PythonMutableHistoryPack,
};

mod exceptions {
    use super::*;

    py_exception!(edenapi, CredsError);
    py_exception!(edenapi, ConfigError);
    py_exception!(edenapi, ResponseError);
    py_exception!(edenapi, CurlError);
    py_exception!(edenapi, HttpError);
    py_exception!(edenapi, SerializationError);
    py_exception!(edenapi, StoreError);
    py_exception!(edenapi, TlsError);
    py_exception!(edenapi, UrlError);
}

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "edenapi"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<client>(py)?;
    Ok(m)
}

fn get_deltastore(py: Python, store: PyObject) -> PyResult<Box<dyn MutableDeltaStore + Send>> {
    if let Ok(store) = store.extract::<mutabledeltastore>(py) {
        Ok(Box::new(store))
    } else {
        Ok(Box::new(PythonMutableDataPack::new(store)?))
    }
}

fn get_historystore(py: Python, store: PyObject) -> PyResult<Box<dyn MutableHistoryStore + Send>> {
    if let Ok(store) = store.extract::<mutablehistorystore>(py) {
        Ok(Box::new(store))
    } else {
        Ok(Box::new(PythonMutableHistoryPack::new(store)?))
    }
}

py_class!(class client |py| {
    data inner: EdenApiCurlClient;

    def __new__(
        _cls,
        url: &PyBytes,
        repo: &PyBytes,
        creds: Option<(&PyBytes, &PyBytes)> = None,
        databatchsize: Option<usize> = None,
        historybatchsize: Option<usize> = None,
        validate: bool = true,
        streamdata: bool = false,
        streamhistory: bool = false,
        streamtrees: bool = false
    ) -> PyResult<client> {
        let url = str::from_utf8(url.data(py)).map_pyerr::<exc::UnicodeDecodeError>(py)?;
        let repo = str::from_utf8(repo.data(py)).map_pyerr::<exc::UnicodeDecodeError>(py)?;

        let mut config = Config::new()
            .base_url_str(url)
            .map_pyerr::<exc::RuntimeError>(py)?
            .repo(repo)
            .data_batch_size(databatchsize)
            .history_batch_size(historybatchsize)
            .validate(validate)
            .stream_data(streamdata)
            .stream_history(streamhistory)
            .stream_trees(streamtrees);

        if let Some((cert, key)) = creds {
            let cert = local_bytes_to_path(cert.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
            let key = local_bytes_to_path(key.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
            config = config.client_creds(cert, key).map_pyerr::<exc::RuntimeError>(py)?;
        }

        let inner = EdenApiCurlClient::new(config).map_pyerr::<exc::RuntimeError>(py)?;
        client::create_instance(py, inner)
    }

    def health_check(&self) -> PyResult<PyObject> {
        Ok(self.inner(py).health_check().map(|()| py.None()).map_err(|e| into_exception(py, e))?)
    }

    def hostname(&self) -> PyResult<String> {
        Ok(self.inner(py).hostname().map_err(|e| into_exception(py, e))?)
    }

    def get_files(
        &self,
        keys: Vec<(PyBytes, PyBytes)>,
        store: PyObject,
        progress_fn: Option<PyObject> = None
    ) -> PyResult<downloadstats> {
        let keys = keys.into_iter()
            .map(|(path, node)| make_key(py, &path, &node))
            .collect::<PyResult<Vec<Key>>>()?;

        let mut store = get_deltastore(py, store)?;

        let client = self.inner(py);
        let progress_fn = progress_fn.map(wrap_callback);
        let stats = py.allow_threads(move || {
            client.get_files(keys, &mut store, progress_fn)
        }).map_err(|e| into_exception(py, e))?;

        downloadstats::create_instance(py, stats)
    }

    def get_history(
        &self,
        keys: Vec<(PyBytes, PyBytes)>,
        store: PyObject,
        depth: Option<u32> = None,
        progress_fn: Option<PyObject> = None
    ) -> PyResult<downloadstats> {
        let keys = keys.into_iter()
            .map(|(path, node)| make_key(py, &path, &node))
            .collect::<PyResult<Vec<Key>>>()?;

        let mut store = get_historystore(py, store)?;

        let client = self.inner(py);
        let progress_fn = progress_fn.map(wrap_callback);
        let stats = py.allow_threads(move || {
            client.get_history(keys, &mut store, depth, progress_fn)
        }).map_err(|e| into_exception(py, e))?;

        downloadstats::create_instance(py, stats)
    }

    def get_trees(
        &self,
        keys: Vec<(PyBytes, PyBytes)>,
        store: PyObject,
        progress_fn: Option<PyObject> = None
    ) -> PyResult<downloadstats> {
        let keys = keys.into_iter()
            .map(|(path, node)| make_key(py, &path, &node))
            .collect::<PyResult<Vec<Key>>>()?;

        let mut store = get_deltastore(py, store)?;

        let client = self.inner(py);
        let progress_fn = progress_fn.map(wrap_callback);
        let stats = py.allow_threads(move || {
            client.get_trees(keys, &mut store, progress_fn)
        }).map_err(|e| into_exception(py, e))?;

        downloadstats::create_instance(py, stats)
    }

    def prefetch_trees(
        &self,
        rootdir: PyBytes,
        mfnodes: Vec<PyBytes>,
        basemfnodes: Vec<PyBytes>,
        store: PyObject,
        depth: Option<usize> = None,
        progress_fn: Option<PyObject> = None
    )  -> PyResult<downloadstats> {
        let rootdir = make_path(py, &rootdir)?;
        let mfnodes = mfnodes
            .into_iter()
            .map(|node| make_node_from_bytes(py, &node))
            .collect::<PyResult<Vec<_>>>()?;
        let basemfnodes = basemfnodes
            .into_iter()
            .map(|node| make_node_from_bytes(py, &node))
            .collect::<PyResult<Vec<_>>>()?;

        let mut store = get_deltastore(py, store)?;

        let client = self.inner(py);
        let progress_fn = progress_fn.map(wrap_callback);
        let stats = py.allow_threads(move || {
            client.prefetch_trees(rootdir, mfnodes, basemfnodes, depth, &mut store, progress_fn)
        }).map_err(|e| into_exception(py, e))?;

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

fn make_key(py: Python, path: &PyBytes, node: &PyBytes) -> PyResult<Key> {
    let path = make_path(py, path)?;
    let node = make_node_from_utf8(py, node)?;
    Ok(Key::new(path, node))
}

fn make_node_from_utf8(py: Python, node: &PyBytes) -> PyResult<Node> {
    let node = str::from_utf8(node.data(py)).map_pyerr::<exc::UnicodeDecodeError>(py)?;
    Ok(Node::from_str(node).map_pyerr::<exc::RuntimeError>(py)?)
}

fn make_node_from_bytes(py: Python, node: &PyBytes) -> PyResult<Node> {
    Ok(Node::from_slice(node.data(py)).map_pyerr::<exc::ValueError>(py)?)
}

fn make_path(py: Python, path: &PyBytes) -> PyResult<RepoPathBuf> {
    Ok(RepoPath::from_utf8(path.data(py))
        .map_pyerr::<exc::ValueError>(py)?
        .to_owned())
}

fn wrap_callback(callback: PyObject) -> ProgressFn {
    Box::new(move |stats: ProgressStats| {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let _ = callback.call(py, stats.as_tuple(), None);
    })
}

fn into_exception(py: Python, error: ApiError) -> PyErr {
    use exceptions::*;
    use ApiErrorKind::*;

    let msg = format!("{}", &error);
    match error.kind() {
        &BadCreds(..) => PyErr::new::<CredsError, _>(py, msg),
        &BadConfig(..) => PyErr::new::<ConfigError, _>(py, msg),
        &BadResponse => PyErr::new::<ResponseError, _>(py, msg),
        &Curl => PyErr::new::<CurlError, _>(py, msg),
        &Http { .. } => PyErr::new::<HttpError, _>(py, msg),
        &Serialization => PyErr::new::<SerializationError, _>(py, msg),
        &Store => PyErr::new::<StoreError, _>(py, msg),
        &Url => PyErr::new::<UrlError, _>(py, msg),
        &Tls => PyErr::new::<TlsError, _>(py, msg),
        &Other(..) => PyErr::new::<exc::RuntimeError, _>(py, msg),
    }
}
