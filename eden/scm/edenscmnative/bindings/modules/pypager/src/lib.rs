/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use clidispatch::io::IO;
use cpython::*;
use cpython_ext::{PyNone, ResultPyErrExt};
use pyconfigparser::config as PyConfig;
use std::cell::{Cell, RefCell};

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "pager"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<pager>(py)?;
    Ok(m)
}

py_class!(class pager |py| {
    data io: RefCell<IO>;
    data closed: Cell<bool>;

    def __new__(_cls, config: PyConfig) -> PyResult<pager> {
        let mut io = IO::stdio();
        let config = &config.get_cfg(py);
        io.start_pager(config).map_pyerr(py)?;
        Self::create_instance(
            py,
            RefCell::new(io),
            Cell::new(false),
        )
    }

    /// Write to pager's main buffer. Text should be in utf-8.
    def write(&self, bytes: PyBytes) -> PyResult<PyNone> {
        self.check_closed(py)?;
        self.io(py).borrow_mut().write(bytes.data(py)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Write to pager's stderr buffer. Text should be in utf-8.
    def write_err(&self, bytes: PyBytes) -> PyResult<PyNone> {
        self.check_closed(py)?;
        self.io(py).borrow_mut().write_err(bytes.data(py)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Write to pager's progress buffer.  Text should be in utf-8.
    def write_progress(&self, bytes: PyBytes) -> PyResult<PyNone> {
        self.check_closed(py)?;
        self.io(py).borrow_mut().write_progress(bytes.data(py)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Wait for the pager to end.
    def close(&self) -> PyResult<PyNone> {
        self.closed(py).set(true);
        // Drop values by replacing them. This sends EOF to the pager.
        *self.io(py).borrow_mut() = IO::stdio();
        Ok(PyNone)
    }
});

impl pager {
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
