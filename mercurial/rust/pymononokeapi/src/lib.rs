// Copyright 2018 Facebook, Inc.
//! pymononokeapi - Python bindings for the Rust `mononokeapi` crate

use cpython::*;
use cpython_failure::ResultPyErrExt;
use encoding::local_bytes_to_path;
use mononokeapi::{MononokeApi, MononokeClient};

use std::str;

py_module_initializer!(
    pymononokeapi,        // module name
    initpymononokeapi,    // py2 init name
    PyInit_pymononokeapi, // py3 init name
    |py, m| {
        // init function
        m.add_class::<PyMononokeClient>(py)?;
        Ok(())
    }
);

py_class!(class PyMononokeClient |py| {
    data client: MononokeClient;

    def __new__(
        _cls,
        host: &PyBytes,
        creds: Option<&PyBytes> = None
    ) -> PyResult<PyMononokeClient> {
        let host = str::from_utf8(host.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
        let creds = match creds {
            Some(path) => Some(local_bytes_to_path(path.data(py)).map_pyerr::<exc::RuntimeError>(py)?),
            None => None,
        };
        let client = MononokeClient::new(host, creds).map_pyerr::<exc::RuntimeError>(py)?;
        PyMononokeClient::create_instance(py, client)
    }

    def health_check(&self) -> PyResult<PyObject> {
        self.client(py).health_check()
            .map(|()| py.None())
            .map_pyerr::<exc::RuntimeError>(py)
    }
});
