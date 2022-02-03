/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::PyNone;
use cpython_ext::PyPathBuf;
use pyconfigparser::config;

extern crate repo as rsrepo;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "repo"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<repo>(py)?;
    Ok(m)
}

py_class!(pub class repo |py| {
    @staticmethod
    def initialize(path: PyPathBuf, config: &config) -> PyResult<PyNone> {
        let mut config = config.get_cfg(py);
        let repopath = path.as_path();
        rsrepo::repo::Repo::init(repopath, &mut config).map_pyerr(py)?;
        Ok(PyNone)
    }
});
