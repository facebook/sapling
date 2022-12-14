/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::Cell;
use std::cell::RefCell;

use clidispatch::io::IO as RustIO;
use cpython::*;
use cpython_ext::wrap_rust_write;
use cpython_ext::PyNone;
use cpython_ext::ResultPyErrExt;
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
