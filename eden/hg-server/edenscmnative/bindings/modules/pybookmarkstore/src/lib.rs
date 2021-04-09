/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;

use cpython::*;

use ::bookmarkstore::BookmarkStore;
use cpython_ext::{PyNone, PyPath};
use types::hgid::HgId;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "bookmarkstore"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<bookmarkstore>(py)?;
    Ok(m)
}

py_class!(class bookmarkstore |py| {
    data bm_store: RefCell<BookmarkStore>;

    def __new__(_cls, path: &PyPath) -> PyResult<bookmarkstore> {
        let bm_store = {
            BookmarkStore::new(path.as_path())
                .map_err(|e| PyErr::new::<exc::IOError, _>(py, format!("{}", e)))?
        };
        bookmarkstore::create_instance(py, RefCell::new(bm_store))
    }

    def update(&self, bookmark: &str, node: PyBytes) -> PyResult<PyNone> {
        let mut bm_store = self.bm_store(py).borrow_mut();
        let hgid = HgId::from_slice(node.data(py))
            .map_err(|e| PyErr::new::<exc::ValueError, _>(py, format!("{}", e)))?;

        bm_store.update(bookmark, hgid)
            .map_err(|e| PyErr::new::<exc::ValueError, _>(py, format!("{}", e)))?;

        Ok(PyNone)
    }

    def remove(&self, bookmark: &str) -> PyResult<PyNone> {
        let mut bm_store = self.bm_store(py).borrow_mut();

        bm_store.remove(bookmark)
            .map_err(|e| PyErr::new::<exc::KeyError, _>(py, format!("{}", e)))?;
        Ok(PyNone)
    }

    def lookup_bookmark(&self, bookmark: &str) -> PyResult<Option<PyBytes>> {
        let bm_store = self.bm_store(py).borrow();

        match bm_store.lookup_bookmark(bookmark) {
            Some(node) => Ok(Some(PyBytes::new(py, node.as_ref()))),
            None => Ok(None),
        }
    }

    def lookup_node(&self, node: PyBytes) -> PyResult<Option<PyList>> {
        let bm_store = self.bm_store(py).borrow();
        let hgid = HgId::from_slice(node.data(py))
            .map_err(|e| PyErr::new::<exc::ValueError, _>(py, format!("{}", e)))?;

        match bm_store.lookup_hgid(&hgid) {
            Some(bms) => {
                let bms: Vec<_> = bms.iter()
                    .map(|bm| PyString::new(py, bm).into_object())
                    .collect();
                Ok(Some(PyList::new(py, bms.as_slice())))
            }
            None => Ok(None),
        }
    }

    def flush(&self) -> PyResult<PyNone> {
        let mut bm_store = self.bm_store(py).borrow_mut();
        bm_store
            .flush()
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, format!("{}", e)))?;
        Ok(PyNone)
    }
});
