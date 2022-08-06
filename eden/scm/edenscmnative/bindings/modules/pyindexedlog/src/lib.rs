/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;

use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::ResultPyErrExt;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "indexedlog"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<Log>(py)?;
    Ok(m)
}

py_class!(class Log |py| {
    data log: RefCell<indexedlog::log::Log>;

    def __new__(_cls, path: &PyPath) -> PyResult<Self> {
        let index_defs = Vec::new();
        let log = indexedlog::log::Log::open(path, index_defs).map_pyerr(py)?;
        Self::create_instance(py, RefCell::new(log))
    }

    /// Get all entries in the Log.
    def entries(&self, skip: usize=0, take: usize=usize::MAX) -> PyResult<Vec<pybytes::Bytes>> {
        let log = self.log(py).borrow();
        let iter = log.iter();
        let items: Vec<&[u8]> = iter.skip(skip).take(take).collect::<Result<Vec<_>, _>>().map_pyerr(py)?;
        let items: Vec<pybytes::Bytes> = items.into_iter().map(|s| {
            pybytes::Bytes::from_bytes(py, log.slice_to_bytes(s))
        }).collect::<Result<_, _>>()?;
        Ok(items)
    }

    /// Append an entry to the Log.
    def append(&self, data: PyBytes) -> PyResult<PyNone> {
        let mut log = self.log(py).borrow_mut();
        let data = data.data(py);
        log.append(data).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Write pending changes to disk and pick up changes from disk.
    def sync(&self) -> PyResult<PyNone> {
        let mut log = self.log(py).borrow_mut();
        log.sync().map_pyerr(py)?;
        Ok(PyNone)
    }
});
