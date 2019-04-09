// Copyright 2018 Facebook, Inc.
//! pyedenapi - Python bindings for the Rust `edenapi` crate

#![allow(non_camel_case_types)]

use std::str;

use cpython::*;
use cpython_failure::ResultPyErrExt;
use encoding::{local_bytes_to_path, path_to_local_bytes};
use failure::format_err;
use log;

use ::edenapi::{Config, EdenApi, EdenApiCurlClient, EdenApiHyperClient};
use types::{Key, Node};

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "edenapi"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<client>(py)?;
    Ok(m)
}

py_class!(class client |py| {
    data inner: Box<dyn EdenApi>;

    def __new__(
        _cls,
        base_url: &PyBytes,
        cache_path: &PyBytes,
        repo: &PyBytes,
        backend: &PyBytes,
        client_creds: Option<(&PyBytes, &PyBytes)> = None,
        batch_size: Option<usize> = None
    ) -> PyResult<client> {
        let base_url = str::from_utf8(base_url.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
        let cache_path = local_bytes_to_path(&cache_path.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
        let repo = str::from_utf8(repo.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
        let backend = str::from_utf8(backend.data(py)).map_pyerr::<exc::RuntimeError>(py)?;

        let mut config = Config::new()
            .base_url_str(base_url)
            .map_pyerr::<exc::RuntimeError>(py)?
            .cache_path(cache_path)
            .repo(repo)
            .batch_size(batch_size);

        if let Some((cert, key)) = client_creds {
            let cert = local_bytes_to_path(cert.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
            let key = local_bytes_to_path(key.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
            config = config.client_creds(cert, key);
        }

        let inner = match backend {
            "curl" => {
                log::debug!("Using curl-backed Eden API client");
                let client = EdenApiCurlClient::new(config).map_pyerr::<exc::RuntimeError>(py)?;
                Box::new(client) as Box<dyn EdenApi>
            },
            "hyper" => {
                log::debug!("Using hyper-backed Eden API client");
                let client = EdenApiHyperClient::new(config).map_pyerr::<exc::RuntimeError>(py)?;
                Box::new(client) as Box<dyn EdenApi>
            },
            invalid => {
                return Err(format_err!("Invalid Eden API backend: {}", invalid)).map_pyerr::<exc::RuntimeError>(py);
            },
        };

        client::create_instance(py, inner)
    }

    def health_check(&self) -> PyResult<PyObject> {
        self.inner(py).health_check()
            .map(|()| py.None())
            .map_pyerr::<exc::RuntimeError>(py)
    }

    def get_files(&self, keys: Vec<(PyBytes, PyBytes)>) -> PyResult<PyBytes> {
        let keys = keys.into_iter()
            .map(|(path, node)| make_key(py, &path, &node))
            .collect::<PyResult<Vec<Key>>>()?;
        let out_path = self.inner(py)
            .get_files(keys)
            .map_pyerr::<exc::RuntimeError>(py)?;
        let out_path = path_to_local_bytes(&out_path)
            .map_pyerr::<exc::RuntimeError>(py)?;
        Ok(PyBytes::new(py, &out_path))

    }

    def get_history(
        &self,
        keys: Vec<(PyBytes, PyBytes)>,
        depth: Option<u32> = None
    ) -> PyResult<PyBytes> {
        let keys = keys.into_iter()
            .map(|(path, node)| make_key(py, &path, &node))
            .collect::<PyResult<Vec<Key>>>()?;
        let out_path = self.inner(py)
            .get_history(keys, depth)
            .map_pyerr::<exc::RuntimeError>(py)?;
        let out_path = path_to_local_bytes(&out_path)
            .map_pyerr::<exc::RuntimeError>(py)?;
        Ok(PyBytes::new(py, &out_path))
    }
});

fn make_key(py: Python, path: &PyBytes, node: &PyBytes) -> PyResult<Key> {
    let path = path.data(py).to_vec();
    let node = str::from_utf8(node.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
    let node = Node::from_str(node).map_pyerr::<exc::RuntimeError>(py)?;
    Ok(Key::new(path, node))
}
