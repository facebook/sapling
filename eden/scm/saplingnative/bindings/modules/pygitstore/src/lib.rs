/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::sync::Arc;

use ::gitstore::git2;
use ::gitstore::GitStore;
use configmodel::Config;
use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::convert::Serde;
use cpython_ext::PyPath;
use cpython_ext::ResultPyErrExt;
use storemodel::types::HgId;
use types::fetch_mode::FetchMode;

mod impl_into;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "gitstore"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<gitstore>(py)?;
    impl_into::register(py);
    Ok(m)
}

py_class!(pub class gitstore |py| {
    data inner: Arc<GitStore>;

    def __new__(_cls, gitdir: &PyPath, config: ImplInto<Arc<dyn Config>>) -> PyResult<Self> {
        let store = GitStore::open(gitdir.as_path(), &config.into()).map_pyerr(py)?;
        Self::create_instance(py, Arc::new(store))
    }

    /// readobj(node, kind="any", mode="AllowRemote") -> bytes.
    /// Read a git object of the given type.
    def readobj(&self, node: Serde<HgId>, kind: &str = "any", mode: Serde<FetchMode> = Serde(FetchMode::AllowRemote)) -> PyResult<PyBytes> {
        let kind = str_to_object_type(py, kind)?;
        let data = self.inner(py).read_obj(node.0, kind, mode.0).map_pyerr(py)?;
        Ok(PyBytes::new(py, &data))
    }

    /// readobjsize(node, kind="any") -> int.
    /// Read a git object size without reading its full content.
    def readobjsize(&self, node: Serde<HgId>, kind: &str = "any") -> PyResult<usize> {
        let kind = str_to_object_type(py, kind)?;
        let size = self.inner(py).read_obj_size(node.0, kind).map_pyerr(py)?;
        Ok(size)
    }

    /// writeobj(kind, data) -> node.
    /// Write object to the store. Not buffered in memory.
    /// Returns the SHA1 hash.
    def writeobj(&self, kind: &str, data: PyBytes) -> PyResult<Serde<HgId>> {
        let kind = str_to_object_type(py, kind)?;
        let node = self.inner(py).write_obj(kind, data.data(py)).map_pyerr(py)?;
        Ok(Serde(node))
    }
});

fn str_to_object_type(py: Python, kind: &str) -> PyResult<git2::ObjectType> {
    match git2::ObjectType::from_str(kind) {
        Some(v) => Ok(v),
        None => Err(PyErr::new::<exc::ValueError, _>(
            py,
            format!("invalid kind: {}", kind),
        )),
    }
}
