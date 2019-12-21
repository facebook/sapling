/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use ::zstore::{Id20, Repair, Zstore};
use cpython::*;
use cpython_ext::Bytes;
use cpython_ext::ResultPyErrExt;
use std::cell::RefCell;

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
    def get(&self, id: Bytes) -> PyResult<Option<Bytes>> {
        let id = Id20::from_slice(id.as_ref()).map_pyerr(py)?;
        let store = self.store(py).borrow();
        let data = store.get(id).map_pyerr(py)?;
        Ok(data.map(Bytes::from))
    }

    /// Insert a blob. Return its Id.
    def insert(&self, data: Bytes, delta_bases: Vec<Bytes> = Vec::new()) -> PyResult<Bytes> {
        let mut store = self.store(py).borrow_mut();
        let delta_bases = delta_bases.into_iter()
            .map(|id| Id20::from_slice(id.as_ref()))
            .collect::<Result<Vec<_>, _>>()
            .map_pyerr(py)?;
        let id = store.insert(data.as_ref(), &delta_bases).map_pyerr(py)?;
        Ok(Bytes::from(id.as_ref().to_vec()))
    }

    /// Test if the store contains an id.
    def contains(&self, id: Bytes) -> PyResult<bool> {
        let id = Id20::from_slice(id.as_ref()).map_pyerr(py)?;
        let store = self.store(py).borrow();
        store.contains(id).map_pyerr(py)
    }

    /// Write pending data to disk. Raise if race condition is detected.
    def flush(&self) -> PyResult<u64> {
        self.store(py).borrow_mut().flush().map_pyerr(py)
    }

    def __getitem__(&self, id: Bytes) -> PyResult<Option<Bytes>> {
        self.get(py, id)
    }

    def __contains__(&self, id: Bytes) -> PyResult<bool> {
        self.contains(py, id)
    }

    @staticmethod
    def repair(path: &str) -> PyResult<PyUnicode> {
        py.allow_threads(|| Zstore::repair(path)).map_pyerr(py).map(|s| PyUnicode::new(py, &s))
    }
});
