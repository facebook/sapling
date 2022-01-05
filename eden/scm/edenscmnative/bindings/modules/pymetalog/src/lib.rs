/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::path::Path;
use std::time::SystemTime;

use ::metalog::CommitOptions;
use ::metalog::Id20;
use ::metalog::MetaLog;
use ::metalog::Repair;
use cpython::*;
use cpython_ext::Bytes;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use cpython_ext::Str;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "metalog"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<metalog>(py)?;
    Ok(m)
}

py_class!(pub class metalog |py| {
    data log: RefCell<MetaLog>;
    data fspath: String;

    def __new__(_cls, path: String, root: Option<Bytes> = None) -> PyResult<Self> {
        let root = root.and_then(|s| Id20::from_slice(s.as_ref()).ok());
        let log = MetaLog::open(&path, root).map_pyerr(py)?;
        Self::create_instance(py, RefCell::new(log), path)
    }

    /// List all roots.
    def roots(&self) -> PyResult<Vec<Bytes>> {
        let path = self.fspath(py);
        let root_ids = MetaLog::list_roots(&path).map_pyerr(py)?;
        Ok(root_ids.into_iter().map(|id| Bytes::from(id.as_ref().to_vec())).collect())
    }

    /// Check out a "root".
    def checkout(&self, root: Bytes) -> PyResult<Self> {
        let root = Id20::from_slice(root.as_ref()).map_pyerr(py)?;
        let log = self.log(py).borrow().checkout(root).map_pyerr(py)?;
        let path = self.fspath(py);
        Self::create_instance(py, RefCell::new(log), path.clone())
    }

    /// Compact the metalog at the given path by only keeping the last entry.
    /// Reduce filesystem usage.
    @staticmethod
    def compact(path: String) -> PyResult<PyNone> {
        MetaLog::compact(path).map_pyerr(py)?;
        Ok(PyNone)
    }

    @staticmethod
    def listroots(path: String) -> PyResult<Vec<Bytes>> {
        let root_ids = MetaLog::list_roots(&path).map_pyerr(py)?;
        Ok(root_ids.into_iter().map(|id| Bytes::from(id.as_ref().to_vec())).collect())
    }

    /// Lookup an item by key. Return None if the key does not exist.
    def get(&self, key: &str) -> PyResult<Option<PyBytes>> {
        let log = self.log(py).borrow();
        let data = log.get(key).map_pyerr(py)?;
        Ok(data.map(|data| PyBytes::new(py, &data)))
    }

    /// Set an item. Return the Id of value.
    def set(&self, key: &str, value: Bytes) -> PyResult<Bytes> {
        let mut log = self.log(py).borrow_mut();
        let id = log.set(key, value.as_ref()).map_pyerr(py)?;
        Ok(Bytes::from(id.as_ref().to_vec()))
    }

    /// Remove an item. Does not raise if the key does not exist.
    def remove(&self, key: &str) -> PyResult<PyNone> {
        let mut log = self.log(py).borrow_mut();
        log.remove(key).map_pyerr(py)?;
        Ok(PyNone)
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
        let id = self.log(py).borrow_mut().commit(opts).map_pyerr(py)?;
        Ok(Bytes::from(id.as_ref().to_vec()))
    }

    /// Export to a git respository
    def exportgit(&self, path: String) -> PyResult<PyNone> {
        let log = self.log(py).borrow();
        log.export_git(Path::new(&path)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Test if there are uncommitted changes.
    def isdirty(&self) -> PyResult<bool> {
        Ok(self.log(py).borrow().is_dirty())
    }

    /// Why the change was made.
    def message(&self) -> PyResult<Str> {
        Ok(Str::from(self.log(py).borrow().message().to_string()))
    }

    /// When the change was made.
    def timestamp(&self) -> PyResult<u64> {
        Ok(self.log(py).borrow().timestamp())
    }

    /// The root id.
    def root(&self) -> PyResult<PyBytes> {
        Ok(PyBytes::new(py, self.log(py).borrow().root_id().as_ref()))
    }

    /// Path on the filesystem.
    def path(&self) -> PyResult<PyPathBuf> {
        Ok(PyPath::from_str(self.fspath(py)).to_owned())
    }

    def __getitem__(&self, key: String) -> PyResult<Option<PyBytes>> {
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
    def repair(path: &str) -> PyResult<Str> {
        py.allow_threads(|| MetaLog::repair(path)).map_pyerr(py).map(Into::into)
    }
});

impl self::metalog {
    pub fn metalog_refcell<'a>(&'a self, py: Python<'a>) -> &'a RefCell<MetaLog> {
        self.log(py)
    }
}
