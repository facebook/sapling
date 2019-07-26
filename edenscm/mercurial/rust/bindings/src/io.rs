// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cpython::*;
use std::io;

/// Return a wrapped PyObject that implements Rust Read / Write traits.
pub fn wrap_pyio(pyio: PyObject) -> WrappedIO {
    WrappedIO(pyio)
}

pub struct WrappedIO(PyObject);

impl io::Read for WrappedIO {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let bytes = self
            .0
            .call_method(py, "read", (buf.len(),), None)
            .map_err(convert_ioerr)?
            .extract::<PyBytes>(py)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "read got non-bytes type"))?;
        let bytes = bytes.data(py);
        if bytes.len() > buf.len() {
            let msg = format!("read got {} bytes, expected {}", bytes.len(), buf.len());
            Err(io::Error::new(io::ErrorKind::InvalidData, msg))
        } else {
            buf[..bytes.len()].copy_from_slice(bytes);
            Ok(bytes.len())
        }
    }
}

impl io::Write for WrappedIO {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let len = buf.len();
        let buf = PyBytes::new(py, buf);
        self.0
            .call_method(py, "write", (buf,), None)
            .map_err(convert_ioerr)?;
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        self.0
            .call_method(py, "flush", NoArgs, None)
            .map_err(convert_ioerr)?;
        Ok(())
    }
}

/// Convert a Python `IOError` to Rust `io::Error`.
fn convert_ioerr(mut pyerr: PyErr) -> io::Error {
    let gil = Python::acquire_gil();
    let py = gil.python();
    let pyerr = pyerr.instance(py);
    if let Ok(errno) = pyerr
        .getattr(py, "errno")
        .and_then(|e| e.extract::<i32>(py))
    {
        io::Error::from_raw_os_error(errno)
    } else {
        let msg = match pyerr.repr(py) {
            Ok(s) => s.to_string_lossy(py).into_owned(),
            Err(_) => "PythonError: unknown".to_string(),
        };
        io::Error::new(io::ErrorKind::Other, msg)
    }
}
