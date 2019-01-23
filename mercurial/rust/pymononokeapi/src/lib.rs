// Copyright 2018 Facebook, Inc.
//! pymononokeapi - Python bindings for the Rust `mononokeapi` crate

use cpython::*;
use cpython_failure::ResultPyErrExt;
use encoding::local_bytes_to_path;
use mononokeapi::{MononokeApi, MononokeClient, MononokeClientBuilder};

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
        base_url: &PyBytes,
        client_creds: Option<&PyBytes> = None
    ) -> PyResult<PyMononokeClient> {
        let base_url = str::from_utf8(base_url.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
        let client_creds = match client_creds {
            Some(path) => Some(local_bytes_to_path(path.data(py)).map_pyerr::<exc::RuntimeError>(py)?),
            None => None,
        };

        let client = MononokeClientBuilder::new()
            .base_url_str(base_url)
            .map_pyerr::<exc::RuntimeError>(py)?
            .client_creds_opt(client_creds)
            .build()
            .map_pyerr::<exc::RuntimeError>(py)?;

        PyMononokeClient::create_instance(py, client)
    }

    def health_check(&self) -> PyResult<PyObject> {
        self.client(py).health_check()
            .map(|()| py.None())
            .map_pyerr::<exc::RuntimeError>(py)
    }
});
