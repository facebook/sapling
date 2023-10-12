/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;
use std::io;

use cpython::*;

use crate::error::ResultPyErrExt;
use crate::none::PyNone;

py_class!(pub class PyRustWrite |py| {
    data io: RefCell<Box<dyn ::io::Write + Send>>;

    def write(&self, bytes: PyBytes) -> PyResult<usize> {
        let mut io = self.io(py).borrow_mut();
        let bytes = bytes.data(py);
        io.write_all(bytes).map_pyerr(py)?;
        Ok(bytes.len())
    }

    def flush(&self) -> PyResult<PyNone> {
        let mut io = self.io(py).borrow_mut();
        io.flush().map_pyerr(py)?;
        Ok(PyNone)
    }

    def isatty(&self) -> PyResult<bool> {
        let io = self.io(py).borrow();
        Ok(io.is_tty())
    }

    def isstdin(&self) -> PyResult<bool> {
        let io = self.io(py).borrow();
        Ok(io.is_stdin())
    }

    def isstdout(&self) -> PyResult<bool> {
        let io = self.io(py).borrow();
        Ok(io.is_stdout())
    }

    def isstderr(&self) -> PyResult<bool> {
        let io = self.io(py).borrow();
        Ok(io.is_stderr())
    }

    def close(&self) -> PyResult<PyNone> {
        let mut io = self.io(py).borrow_mut();
        io.flush().map_pyerr(py)?;
        *io = Box::new(ClosedIO);
        Ok(PyNone)
    }
});

struct ClosedIO;

impl io::Write for ClosedIO {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "stream was closed",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "stream was closed",
        ))
    }
}

impl ::io::IsTty for ClosedIO {
    fn is_tty(&self) -> bool {
        false
    }
}

/// Wrap a Rust Write trait object into a Python object.
pub fn wrap_rust_write(py: Python, w: impl ::io::Write + Send + 'static) -> PyResult<PyRustWrite> {
    PyRustWrite::create_instance(py, RefCell::new(Box::new(w)))
}
