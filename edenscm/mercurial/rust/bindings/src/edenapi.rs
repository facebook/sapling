// Copyright 2018 Facebook, Inc.
//! pyedenapi - Python bindings for the Rust `edenapi` crate

#![allow(non_camel_case_types)]

use std::{cell::RefCell, mem, str};

use cpython::*;
use cpython_failure::ResultPyErrExt;

use ::edenapi::{EdenApi, EdenApiHttpClient};
use ::revisionstore::Key;
use encoding::{local_bytes_to_path, path_to_local_bytes};
use types::node::Node;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "edenapi"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<client>(py)?;
    m.add_class::<getfilesrequest>(py)?;
    Ok(m)
}

py_class!(class client |py| {
    data inner: EdenApiHttpClient;

    def __new__(
        _cls,
        base_url: &PyBytes,
        cache_path: &PyBytes,
        repo: &PyBytes,
        client_creds: Option<(&PyBytes, &PyBytes)> = None
    ) -> PyResult<client> {
        let base_url = str::from_utf8(base_url.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
        let cache_path = str::from_utf8(cache_path.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
        let repo = str::from_utf8(repo.data(py)).map_pyerr::<exc::RuntimeError>(py)?;

        let mut builder = EdenApiHttpClient::builder();

        if let Some((cert, key)) = client_creds {
            let cert = local_bytes_to_path(cert.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
            let key = local_bytes_to_path(key.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
            builder = builder.client_creds(cert, key).map_pyerr::<exc::RuntimeError>(py)?;
        }

        let inner = builder.base_url_str(base_url)
            .map_pyerr::<exc::RuntimeError>(py)?
            .cache_path(cache_path)
            .repo(repo)
            .build()
            .map_pyerr::<exc::RuntimeError>(py)?;

        client::create_instance(py, inner)
    }

    def health_check(&self) -> PyResult<PyObject> {
        self.inner(py).health_check()
            .map(|()| py.None())
            .map_pyerr::<exc::RuntimeError>(py)
    }

    def get_files(&self, request: &getfilesrequest) -> PyResult<PyBytes> {
        let mut keys = request.keys(py).try_borrow_mut().map_pyerr::<exc::RuntimeError>(py)?;
        let keys = mem::replace(&mut *keys, Vec::new());

        let out_path = self.inner(py).get_files(keys).map_pyerr::<exc::RuntimeError>(py)?;
        let out_path = path_to_local_bytes(&out_path).map_pyerr::<exc::RuntimeError>(py)?;
        Ok(PyBytes::new(py, &out_path))

    }
});

py_class!(class getfilesrequest |py| {
    data keys: RefCell<Vec<Key>>;

    def __new__(_cls) -> PyResult<getfilesrequest> {
        getfilesrequest::create_instance(py, RefCell::new(Vec::new()))
    }

    def push(&self, node: &PyBytes, path: &PyBytes) -> PyResult<PyObject> {
        let node = str::from_utf8(node.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
        let node = Node::from_str(node).map_pyerr::<exc::RuntimeError>(py)?;
        let path = path.data(py).to_vec();
        let key = Key::new(path, node);

        let mut keys = self.keys(py).try_borrow_mut().map_pyerr::<exc::RuntimeError>(py)?;
        keys.push(key);

        Ok(py.None())
    }
});
