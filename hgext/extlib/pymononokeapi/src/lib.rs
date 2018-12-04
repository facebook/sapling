// Copyright 2018 Facebook, Inc.
//! pymononokeapi - Python bindings for the Rust `mononokeapi` crate

#[macro_use]
extern crate cpython;
extern crate encoding;
extern crate failure;
extern crate http;
extern crate mononokeapi;

mod errors;

use cpython::{PyBytes, PyObject, PyResult};
use encoding::local_bytes_to_path;
use http::Uri;
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
        creds: Option<&PyBytes> = None
    ) -> PyResult<PyMononokeClient> {
        let host = Uri::from_shared(host.data(py).into()).into_pyresult(py)?;
        let creds = match creds {
            Some(path) => Some(local_bytes_to_path(path.data(py)).into_pyresult(py)?),
            None => None,
        };
        let client = MononokeClient::new(host, creds).into_pyresult(py)?;
        PyMononokeClient::create_instance(py, client)
    }

    def health_check(&self) -> PyResult<PyObject> {
        self.client(py).health_check()
            .map(|()| py.None())
            .into_pyresult(py)
    }
});
