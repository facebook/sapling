// Copyright 2018 Facebook, Inc.
//! pymononokeapi - Python bindings for the Rust `mononokeapi` crate

#[macro_use]
extern crate cpython;
extern crate encoding;
extern crate failure;
extern crate mononokeapi;

mod errors;

use cpython::{PyBytes, PyObject, PyResult};
use encoding::local_bytes_to_path;
use mononokeapi::MononokeClient;

use errors::IntoPyResult;

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
        creds: &PyBytes
    ) -> PyResult<PyMononokeClient> {
        let host = host.data(py);
        let creds = local_bytes_to_path(creds.data(py)).into_pyresult(py)?;
        let client = MononokeClient::new(host, creds).into_pyresult(py)?;
        PyMononokeClient::create_instance(py, client)
    }

    def health_check(&self) -> PyResult<PyObject> {
        self.client(py).health_check()
            .map(|()| py.None())
            .into_pyresult(py)
    }
});
