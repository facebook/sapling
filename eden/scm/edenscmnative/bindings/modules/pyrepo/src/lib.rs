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
use cpython_ext::ExtractInner;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use pyconfigparser::config;
use pydag::commits::commits as PyCommits;
use pyedenapi::PyClient;
use pymetalog::metalog as PyMetaLog;
use rsrepo::repo::Repo;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "repo"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<repo>(py)?;
    m.add(
        py,
        "loadchangelog",
        py_fn!(
            py,
            load_changelog(
                dir: &PyPath,
                storerequirements: Vec<String>,
                metalog: PyMetaLog,
                edenapi: Option<PyClient>
            )
        ),
    )?;
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

    def __new__(_cls, path: PyPathBuf, config: &config) -> PyResult<Self> {
        let config = config.get_cfg(py);
        let abs_path = util::path::absolute(path.as_path()).map_pyerr(py)?;
        let repo = Repo::load_with_config(abs_path, config).map_pyerr(py)?;
        Self::create_instance(py, RefCell::new(repo))
    }

    def metalog(&self) -> PyResult<PyMetaLog> {
        let mut repo_ref = self.inner(py).borrow_mut();
        let path = String::from(repo_ref.metalog_path().to_string_lossy());
        let log_ref = repo_ref.metalog().map_pyerr(py)?;
        PyMetaLog::create_instance(py, log_ref, path)
    }

    def invalidatemetalog(&self) -> PyResult<PyNone> {
        let mut repo_ref = self.inner(py).borrow_mut();
        repo_ref.invalidate_metalog();
        Ok(PyNone)
    }
});

fn load_changelog(
    py: Python,
    dir: &PyPath,
    storerequirements: Vec<String>,
    metalog: PyMetaLog,
    edenapi: Option<PyClient>,
) -> PyResult<PyCommits> {
    let client = edenapi.map(|e| e.extract_inner(py));
    let meta = metalog.metalog_rwlock(py);
    let inner = py
        .allow_threads(|| rsrepo::open_dag_commits(dir.as_path(), storerequirements, meta, client))
        .map_pyerr(py)?;
    PyCommits::create_instance(py, RefCell::new(inner))
}
