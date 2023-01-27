/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::collections::HashMap;
use std::sync::Arc;

use ::copytrace::GitCopyTrace;
use ::types::HgId;
use cpython::*;
use cpython_ext::convert::Serde;
use cpython_ext::PyPath;
use cpython_ext::ResultPyErrExt;
use parking_lot::Mutex;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "copytrace"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<gitcopytrace>(py)?;
    Ok(m)
}

py_class!(pub class gitcopytrace |py| {
    data inner: Arc<Mutex<GitCopyTrace>>;

    def __new__(_cls, gitdir: &PyPath) -> PyResult<Self> {
        let copytrace = GitCopyTrace::open(gitdir.as_path()).map_pyerr(py)?;
        Self::create_instance(py, Arc::new(Mutex::new(copytrace)))
    }

    /// Find copies between old and new commits, the result is a {newpath: oldpath} map.
    def findcopies(
        &self, oldnode: Serde<HgId>, newnode: Serde<HgId>
    ) -> PyResult<HashMap<String, String>> {
        let copytrace = self.inner(py).lock();
        let copies = copytrace.find_copies(oldnode.0, newnode.0).map_pyerr(py)?;
        Ok(copies)
    }
});
