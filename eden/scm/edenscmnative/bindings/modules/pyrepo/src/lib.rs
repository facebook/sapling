/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

extern crate repo as rsrepo;

use std::cell::RefCell;

use cpython::*;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::PyNone;
use cpython_ext::PyPathBuf;
use pyconfigparser::config;
use rsrepo::repo::Repo;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "repo"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<repo>(py)?;
    Ok(m)
}

py_class!(pub class repo |py| {
    data inner: RefCell<Repo>;

    @staticmethod
    def initialize(path: PyPathBuf, config: &config) -> PyResult<PyNone> {
        let mut config = config.get_cfg(py);
        let repopath = path.as_path();
        Repo::init(repopath, &mut config).map_pyerr(py)?;
        Ok(PyNone)
    }

    def __new__(_cls, path: PyPathBuf) -> PyResult<Self> {
        let abs_path = util::path::absolute(path.as_path()).map_pyerr(py)?;
        let repo = Repo::load(abs_path).map_pyerr(py)?;
        Self::create_instance(py, RefCell::new(repo))
    }
});
