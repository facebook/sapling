/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::Cell;
use std::fs::File;

use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use fs2::FileExt;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "lock"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<pathlock>(py)?;
    Ok(m)
}

py_class!(class pathlock |py| {
    data lock: Cell<Option<File>>;

    @classmethod def trylock(_cls, dir: PyPathBuf, name: String, contents: String) -> PyResult<pathlock> {
        Self::create_instance(
            py,
            Cell::new(Some(
                repolock::try_lock(dir.as_path(), &name, contents.as_bytes()).map_pyerr(py)?,
            )),
        )
    }

    def unlock(&self) -> PyResult<PyNone> {
        if let Some(f) = self.lock(py).replace(None) {
            f.unlock().map_pyerr(py)?;
            Ok(PyNone)
        } else {
            Err(PyErr::new::<exc::ValueError, _>(py, "lock is already unlocked"))
        }
    }
});
