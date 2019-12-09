/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use ::metalog::{CommitOptions, Id20, MetaLog, Repair};
use cpython::*;
use cpython_ext::Bytes;
use cpython_failure::ResultPyErrExt;
use std::cell::RefCell;
use std::time::SystemTime;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "metalog"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<metalog>(py)?;
    Ok(m)
}

py_class!(class metalog |py| {
    data log: RefCell<MetaLog>;

    def __new__(_cls, path: String, root: Option<Bytes> = None) -> PyResult<Self> {
        let root = root.and_then(|s| Id20::from_slice(s.as_ref()).ok());
        let log = MetaLog::open(&path, root).map_pyerr::<exc::IOError>(py)?;
        Self::create_instance(py, RefCell::new(log))
    }

    @staticmethod
    def listroots(path: String) -> PyResult<Vec<Bytes>> {
        let root_ids = MetaLog::list_roots(&path).map_pyerr::<exc::IOError>(py)?;
        Ok(root_ids.into_iter().map(|id| Bytes::from(id.as_ref().to_vec())).collect())
    }

    /// Lookup an item by key. Return None if the key does not exist.
    def get(&self, key: &str) -> PyResult<Option<Bytes>> {
        let log = self.log(py).borrow();
        let data = log.get(key).map_pyerr::<exc::IOError>(py)?;
        Ok(data.map(Bytes::from))
    }

    /// Set an item. Return the Id of value.
    def set(&self, key: &str, value: Bytes) -> PyResult<Bytes> {
        let mut log = self.log(py).borrow_mut();
        let id = log.set(key, value.as_ref()).map_pyerr::<exc::IOError>(py)?;
        Ok(Bytes::from(id.as_ref().to_vec()))
    }

    /// Remove an item. Does not raise if the key does not exist.
    def remove(&self, key: &str) -> PyResult<PyObject> {
        let mut log = self.log(py).borrow_mut();
        log.remove(key).map_pyerr::<exc::IOError>(py)?;
        Ok(py.None())
    }

    /// Get all keys.
    def keys(&self) -> PyResult<Vec<Bytes>> {
        let keys = self.log(py).borrow()
            .keys().iter().map(|s| Bytes::from(s.as_bytes().to_vec())).collect();
        Ok(keys)
    }

    /// Write pending data to disk. Raise if race condition is detected.
    def commit(&self, message: &str, time: Option<u64> = None, pending: bool = false) -> PyResult<Bytes> {
        let mut opts = CommitOptions::default();
        opts.detached = pending;
        opts.timestamp = time.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs()).unwrap_or(0)
        });
        opts.message = message;
        let id = self.log(py).borrow_mut().commit(opts).map_pyerr::<exc::IOError>(py)?;
        Ok(Bytes::from(id.as_ref().to_vec()))
    }

    /// Test if there are uncommitted changes.
    def isdirty(&self) -> PyResult<bool> {
        Ok(self.log(py).borrow().is_dirty())
    }

    /// Why the change was made.
    def message(&self) -> PyResult<Bytes> {
        Ok(Bytes::from(self.log(py).borrow().message().to_string()))
    }

    /// When the change was made.
    def timestamp(&self) -> PyResult<u64> {
        Ok(self.log(py).borrow().timestamp())
    }

    def __getitem__(&self, key: String) -> PyResult<Option<Bytes>> {
        self.get(py, &key)
    }

    def __setitem__(&self, key: String, value: Bytes) -> PyResult<()> {
        self.set(py, &key, value)?;
        Ok(())
    }

    def __delitem__(&self, key: String) -> PyResult<()> {
        self.remove(py, &key)?;
        Ok(())
    }

    @staticmethod
    def repair(path: &str) -> PyResult<PyUnicode> {
        MetaLog::repair(path).map_pyerr::<exc::IOError>(py).map(|s| PyUnicode::new(py, &s))
    }
});
