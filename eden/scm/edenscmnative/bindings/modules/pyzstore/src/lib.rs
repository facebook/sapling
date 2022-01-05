/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;

use ::zstore::Id20;
use ::zstore::Repair;
use ::zstore::Zstore;
use cpython::*;
use cpython_ext::ResultPyErrExt;
use cpython_ext::Str;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "zstore"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<zstore>(py)?;
    Ok(m)
}

py_class!(class zstore |py| {
    data store: RefCell<Zstore>;

    def __new__(_cls, path: String) -> PyResult<Self> {
        let log = Zstore::open(&path).map_pyerr(py)?;
        Self::create_instance(py, RefCell::new(log))
    }

    /// Lookup a blob by id. Return None if the id is unknown.
    def get(&self, id: PyBytes) -> PyResult<Option<PyBytes>> {
        let id = Id20::from_slice(id.data(py)).map_pyerr(py)?;
        let store = self.store(py).borrow();
        let data = store.get(id).map_pyerr(py)?;
        Ok(data.map(|data| PyBytes::new(py, &data)))
    }

    /// Insert a blob. Return its Id.
    def insert(&self, data: PyBytes, delta_bases: Vec<PyBytes> = Vec::new()) -> PyResult<PyBytes> {
        let mut store = self.store(py).borrow_mut();
        let delta_bases = delta_bases.into_iter()
            .map(|id| Id20::from_slice(id.data(py)))
            .collect::<Result<Vec<_>, _>>()
            .map_pyerr(py)?;
        let id = store.insert(data.data(py), &delta_bases).map_pyerr(py)?;
        Ok(PyBytes::new(py, id.as_ref()))
    }

    /// Test if the store contains an id.
    def contains(&self, id: PyBytes) -> PyResult<bool> {
        let id = Id20::from_slice(id.data(py)).map_pyerr(py)?;
        let store = self.store(py).borrow();
        store.contains(id).map_pyerr(py)
    }

    /// Write pending data to disk. Raise if race condition is detected.
    def flush(&self) -> PyResult<u64> {
        self.store(py).borrow_mut().flush().map_pyerr(py)
    }

    def __getitem__(&self, id: PyBytes) -> PyResult<Option<PyBytes>> {
        self.get(py, id)
    }

    def __contains__(&self, id: PyBytes) -> PyResult<bool> {
        self.contains(py, id)
    }

    @staticmethod
    def repair(path: &str) -> PyResult<Str> {
        py.allow_threads(|| Zstore::repair(path)).map_pyerr(py).map(Into::into)
    }
});
