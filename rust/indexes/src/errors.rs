// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cpython::{Python, PyErr};
use cpython::exc::RuntimeError;

error_chain! {
    foreign_links {
        Radix(::radixbuf::errors::Error);
    }

    errors {
        IndexCorrupted {
            description("corrupted index")
        }
    }
}

/// Convert `Error` to Python `RuntimeError`s
impl From<Error> for PyErr {
    fn from(e: Error) -> PyErr {
        let gil = Python::acquire_gil();
        let py = gil.python();
        PyErr::new::<RuntimeError, _>(py, format!("{}", e))
    }
}
