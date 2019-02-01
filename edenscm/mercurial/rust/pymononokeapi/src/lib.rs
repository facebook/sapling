// Copyright 2018 Facebook, Inc.
//! pymononokeapi - Python bindings for the Rust `mononokeapi` crate

use cpython::*;
use cpython_failure::ResultPyErrExt;
use encoding::{local_bytes_to_path, path_to_local_bytes};
use mononokeapi::{MononokeApi, MononokeClient, MononokeClientBuilder};
use revisionstore::Key;
use types::node::Node;

use std::{cell::RefCell, mem, str};

py_module_initializer!(
    pymononokeapi,        // module name
    initpymononokeapi,    // py2 init name
    PyInit_pymononokeapi, // py3 init name
    |py, m| {
        // init function
        m.add_class::<PyMononokeClient>(py)?;
        m.add_class::<GetFilesRequest>(py)?;
        Ok(())
    }
);

py_class!(class PyMononokeClient |py| {
    data client: MononokeClient;

    def __new__(
        _cls,
        base_url: &PyBytes,
        cache_path: &PyBytes,
        repo: &PyBytes,
        client_creds: Option<&PyBytes> = None
    ) -> PyResult<PyMononokeClient> {
        let base_url = str::from_utf8(base_url.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
        let cache_path = str::from_utf8(cache_path.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
        let repo = str::from_utf8(repo.data(py)).map_pyerr::<exc::RuntimeError>(py)?;

        let mut builder = MononokeClientBuilder::new();

        if let Some(path) = client_creds {
            let path = local_bytes_to_path(path.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
            builder = builder.client_creds(path).map_pyerr::<exc::RuntimeError>(py)?;
        }

        let client = builder.base_url_str(base_url)
            .map_pyerr::<exc::RuntimeError>(py)?
            .cache_path(cache_path)
            .repo(repo)
            .build()
            .map_pyerr::<exc::RuntimeError>(py)?;

        PyMononokeClient::create_instance(py, client)
    }

    def health_check(&self) -> PyResult<PyObject> {
        self.client(py).health_check()
            .map(|()| py.None())
            .map_pyerr::<exc::RuntimeError>(py)
    }

    def get_files(&self, request: &GetFilesRequest) -> PyResult<PyBytes> {
        let mut keys = request.keys(py).try_borrow_mut().map_pyerr::<exc::RuntimeError>(py)?;
        let keys = mem::replace(&mut *keys, Vec::new());

        let out_path = self.client(py).get_files(keys).map_pyerr::<exc::RuntimeError>(py)?;
        let out_path = path_to_local_bytes(&out_path).map_pyerr::<exc::RuntimeError>(py)?;
        Ok(PyBytes::new(py, &out_path))

    }
});

py_class!(class GetFilesRequest |py| {
    data keys: RefCell<Vec<Key>>;

    def __new__(_cls) -> PyResult<GetFilesRequest> {
        GetFilesRequest::create_instance(py, RefCell::new(Vec::new()))
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
