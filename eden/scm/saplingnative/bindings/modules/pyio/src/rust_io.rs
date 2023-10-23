/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::Cell;
use std::cell::RefCell;
use std::io;
use std::io::BufRead;
use std::io::Read;

use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::ResultPyErrExt;

py_class!(pub class PyRustIO |py| {
    data r: RefCell<Option<io::BufReader<Box<dyn ::io::Read + Send>>>>;
    data w: RefCell<Option<Box<dyn ::io::Write + Send>>>;
    data is_closed: Cell<bool>;

    /// Read at most `n` bytes from the input.
    /// If `n` is negative, read everything till the end.
    def read(&self, n: i64 = -1) -> PyResult<PyBytes> {
        let mut io = self.r(py).borrow_mut();
        let io = match io.as_mut() {
            Some(io) => io,
            None => return Err(not_readable(py)),
        };
        let mut buf = Vec::<u8>::new();
        if n < 0 {
            io.read_to_end(&mut buf).map_pyerr(py)?;
        } else if n == 0 {
            // Avoid BufReader::read(), which can block filling its buffer.
        } else {
            buf.resize(n as usize, 0u8);
            let read_bytes = io.read(&mut buf).map_pyerr(py)?;
            buf.truncate(read_bytes);
        }
        Ok(PyBytes::new(py, &buf))
    }

    def readline(&self) -> PyResult<PyBytes> {
        let mut io = self.r(py).borrow_mut();
        let io = match io.as_mut() {
            Some(io) => io,
            None => return Err(not_readable(py)),
        };
        let mut buf = Vec::<u8>::new();
        io.read_until(b'\n', &mut buf).map_pyerr(py)?;
        Ok(PyBytes::new(py, &buf))
    }

    def write(&self, bytes: PyBytes) -> PyResult<usize> {
        let mut io = self.w(py).borrow_mut();
        let io = match io.as_mut() {
            Some(io) => io,
            None => return Err(not_writable(py)),
        };
        let bytes = bytes.data(py);
        io.write_all(bytes).map_pyerr(py)?;
        Ok(bytes.len())
    }

    def flush(&self) -> PyResult<PyNone> {
        let mut io = self.w(py).borrow_mut();
        let io = match io.as_mut() {
            Some(io) => io,
            None => return Ok(PyNone),
        };
        io.flush().map_pyerr(py)?;
        Ok(PyNone)
    }

    def isatty(&self) -> PyResult<bool> {
        let io = self.w(py).borrow();
        match io.as_ref() {
            Some(io) => Ok(io.is_tty()),
            None => {
                let io = self.r(py).borrow();
                match io.as_ref() {
                    Some(io) => Ok(io.get_ref().is_tty()),
                    None => Ok(false),
                }
            }
        }
    }

    def isstdin(&self) -> PyResult<bool> {
        let io = self.r(py).borrow();
        match io.as_ref() {
            Some(io) => Ok(io.get_ref().is_stdin()),
            None => Ok(false),
        }
    }

    def isstdout(&self) -> PyResult<bool> {
        let io = self.w(py).borrow();
        match io.as_ref() {
            Some(io) => Ok(io.is_stdout()),
            None => Ok(false),
        }
    }

    def isstderr(&self) -> PyResult<bool> {
        let io = self.w(py).borrow();
        match io.as_ref() {
            Some(io) => Ok(io.is_stderr()),
            None => Ok(false),
        }
    }

    def close(&self) -> PyResult<PyNone> {
        let mut io = self.w(py).borrow_mut();
        if let Some(inner_io) = io.as_mut() {
            inner_io.flush().map_pyerr(py)?;
            *io = Some(Box::new(ClosedIO));
        };
        let mut io = self.r(py).borrow_mut();
        if io.is_some() {
            *io = Some(io::BufReader::new(Box::new(ClosedIO)));
        }
        self.is_closed(py).set(true);
        Ok(PyNone)
    }

    @property
    def closed(&self) -> PyResult<bool> {
        Ok(self.is_closed(py).get())
    }

    def readable(&self) -> PyResult<bool> {
        Ok(self.r(py).borrow().is_some())
    }

    def seekable(&self) -> PyResult<bool> {
        Ok(false)
    }

    def writable(&self) -> PyResult<bool> {
        Ok(self.w(py).borrow().is_some())
    }

    def fileno(&self) -> PyResult<usize> {
        // Emulated. Might be inaccurate.
        if self.isstdin(py)? {
            Ok(0)
        } else if self.isstdout(py)? {
            Ok(1)
        } else if self.isstderr(py)? {
            Ok(2)
        } else {
            Err(PyErr::from_instance(py, py.import("io")?.get(py, "UnsupportedOperation")?))
        }
    }
});

fn not_readable(py: Python) -> PyErr {
    PyErr::new::<exc::IOError, _>(py, "stream is not readable")
}

fn not_writable(py: Python) -> PyErr {
    PyErr::new::<exc::IOError, _>(py, "stream is not writable")
}

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

impl io::Read for ClosedIO {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
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
pub(crate) fn wrap_rust_write(
    py: Python,
    w: impl ::io::Write + Send + 'static,
) -> PyResult<PyRustIO> {
    PyRustIO::create_instance(
        py,
        RefCell::new(None),
        RefCell::new(Some(Box::new(w))),
        Cell::new(false),
    )
}

/// Wrap a Rust Read trait object into a Python object.
pub(crate) fn wrap_rust_read(
    py: Python,
    r: impl ::io::Read + Send + 'static,
) -> PyResult<PyRustIO> {
    PyRustIO::create_instance(
        py,
        RefCell::new(Some(io::BufReader::new(Box::new(r)))),
        RefCell::new(None),
        Cell::new(false),
    )
}
