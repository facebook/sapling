/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::Cell;
use std::cell::RefCell;

use cpython::*;
use cpython_ext::wrap_rust_write;
use cpython_ext::PyNone;
use cpython_ext::ResultPyErrExt;
use io::time_interval;
use io::IO as RustIO;
use pyconfigloader::config as PyConfig;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "io"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<IO>(py)?;
    m.add_class::<styler>(py)?;
    m.add(
        py,
        "shouldcolor",
        py_fn!(py, should_color(config: PyConfig)),
    )?;
    Ok(m)
}

py_class!(class IO |py| {
    data closed: Cell<bool>;

    @staticmethod
    def main() -> PyResult<IO> {
        Self::create_instance(py, Cell::new(false))
    }

    /// Start the stream pager.
    def start_pager(&self, config: PyConfig) -> PyResult<PyNone> {
        self.check_closed(py)?;
        let io = RustIO::main().map_pyerr(py)?;
        let config = &config.get_cfg(py);
        io.start_pager(config).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Test if the pager is active.
    def is_pager_active(&self) -> PyResult<bool> {
        let io = RustIO::main().map_pyerr(py)?;
        Ok(io.is_pager_active())
    }

    /// Write to pager's main buffer. Text should be in utf-8.
    def write(&self, bytes: PyBytes) -> PyResult<PyNone> {
        self.check_closed(py)?;
        let io = RustIO::main().map_pyerr(py)?;
        io.write(bytes.data(py)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Write to pager's stderr buffer. Text should be in utf-8.
    def write_err(&self, bytes: PyBytes) -> PyResult<PyNone> {
        self.check_closed(py)?;
        let io = RustIO::main().map_pyerr(py)?;
        io.write_err(bytes.data(py)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Set the progress content.
    def set_progress(&self, message: &str) -> PyResult<PyNone> {
        self.check_closed(py)?;
        let io = RustIO::main().map_pyerr(py)?;
        io.set_progress_str(message).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Wait for the pager to end.
    def wait_pager(&self) -> PyResult<PyNone> {
        self.closed(py).set(true);
        let io = RustIO::main().map_pyerr(py)?;
        io.wait_pager().map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Return the output stream as a Python object with "write" method.
    def output(&self) -> PyResult<PyObject> {
        let io = RustIO::main().map_pyerr(py)?;
        Ok(wrap_rust_write(py, io.output())?.into_object())
    }

    /// Return the error stream as a Python object with "write" method.
    def error(&self) -> PyResult<PyObject> {
        let io = RustIO::main().map_pyerr(py)?;
        Ok(wrap_rust_write(py, io.error())?.into_object())
    }

    /// Flush pending changes.
    def flush(&self) -> PyResult<PyNone> {
        let io = RustIO::main().map_pyerr(py)?;
        io.flush().map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Disable progress output.
    def disable_progress(&self, disabled: bool = true) -> PyResult<PyNone> {
        let io = RustIO::main().map_pyerr(py)?;
        io.disable_progress(disabled).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Get a context object to track "blocked time"
    def scoped_blocked_interval(&self, name: String) -> PyResult<ScopedBlockedInterval> {
        let io = RustIO::main().map_pyerr(py)?;
        let interval = io.time_interval().scoped_blocked_interval(name.into());
        ScopedBlockedInterval::create_instance(py, RefCell::new(Some(interval)))
    }
});

py_class!(class ScopedBlockedInterval |py| {
    data inner: RefCell<Option<time_interval::BlockedInterval>>;

    def __enter__(&self) -> PyResult<Self> {
        Ok(self.clone_ref(py))
    }

    def __exit__(&self, _ty: Option<PyType>, _value: PyObject, _traceback: PyObject) -> PyResult<bool> {
        let _interval = self.inner(py).borrow_mut().take();
        Ok(false) // preserves exceptions
    }
});

impl IO {
    fn check_closed(&self, py: Python) -> PyResult<PyNone> {
        if self.closed(py).get() {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "pager was closed",
            ))
            .map_pyerr(py)
        } else {
            Ok(PyNone)
        }
    }
}

py_class!(class styler |py| {
    data inner: RefCell<termstyle::Styler>;

    def __new__(_cls, colors: u32) -> PyResult<styler> {
        let level = if colors >= 16777216 {
            termstyle::ColorLevel::TrueColor
        } else if colors >= 256 {
            termstyle::ColorLevel::TwoFiftySix
        } else {
            termstyle::ColorLevel::Sixteen
        };

        Self::create_instance(py, RefCell::new(termstyle::Styler::from_level(level).map_pyerr(py)?))
    }

    def renderbytes(&self, style: &str, text: &str)  -> PyResult<PyBytes> {
        Ok(PyBytes::new(
            py,
            self.inner(py).borrow_mut().render_bytes(style, text)
                .map_pyerr(py)?
                .as_ref(),
        ))
    }
});

fn should_color(py: Python, config: PyConfig) -> PyResult<bool> {
    let config = &config.get_cfg(py);
    Ok(termstyle::should_color(
        config,
        &RustIO::main().map_pyerr(py)?.output(),
    ))
}

/// Wrap a PyObject into a Rust object that implements Rust Read / Write traits.
pub fn wrap_pyio(pyio: PyObject) -> WrappedIO {
    WrappedIO(pyio)
}

pub struct WrappedIO(pub PyObject);

impl std::io::Read for WrappedIO {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let bytes = self
            .0
            .call_method(py, "read", (buf.len(),), None)
            .map_err(convert_ioerr)?
            .extract::<PyBytes>(py)
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "read got non-bytes type")
            })?;
        let bytes = bytes.data(py);
        if bytes.len() > buf.len() {
            let msg = format!("read got {} bytes, expected {}", bytes.len(), buf.len());
            Err(std::io::Error::new(std::io::ErrorKind::InvalidData, msg))
        } else {
            buf[..bytes.len()].copy_from_slice(bytes);
            Ok(bytes.len())
        }
    }
}

impl std::io::Write for WrappedIO {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let len = buf.len();
        let buf = PyBytes::new(py, buf);
        self.0
            .call_method(py, "write", (buf,), None)
            .map_err(convert_ioerr)?;
        Ok(len)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        self.0
            .call_method(py, "flush", NoArgs, None)
            .map_err(convert_ioerr)?;
        Ok(())
    }
}

impl ::io::IsTty for WrappedIO {
    fn is_tty(&self) -> bool {
        (|| -> PyResult<bool> {
            let gil = Python::acquire_gil();
            let py = gil.python();
            let result = self.0.call_method(py, "isatty", NoArgs, None)?;
            result.extract(py)
        })()
        .unwrap_or(false)
    }
}

/// Convert a Python `IOError` to Rust `std::io::Error`.
fn convert_ioerr(mut pyerr: PyErr) -> std::io::Error {
    let gil = Python::acquire_gil();
    let py = gil.python();
    let pyerr = pyerr.instance(py);
    if let Ok(errno) = pyerr
        .getattr(py, "errno")
        .and_then(|e| e.extract::<i32>(py))
    {
        std::io::Error::from_raw_os_error(errno)
    } else {
        let msg = match pyerr.repr(py) {
            Ok(s) => s.to_string_lossy(py).into_owned(),
            Err(_) => "PythonError: unknown".to_string(),
        };
        std::io::Error::new(std::io::ErrorKind::Other, msg)
    }
}
