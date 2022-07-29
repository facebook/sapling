/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

extern crate repo as rsrepo;

use cpython::*;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::PyNone;
use cpython_ext::PyPathBuf;
use parking_lot::RwLock;
use pyconfigparser::config;
use pydag::commits::commits as PyCommits;
use pyedenapi::PyClient as PyEdenApi;
use pymetalog::metalog as PyMetaLog;
use rsrepo::repo::Repo;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "repo"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<repo>(py)?;
    Ok(m)
}

py_class!(pub class repo |py| {
    data inner: RwLock<Repo>;

    @staticmethod
    def initialize(path: PyPathBuf, config: &config) -> PyResult<PyNone> {
        Repo::init(path.as_path(), &config.get_cfg(py), None, &[]).map_pyerr(py)?;
        Ok(PyNone)
    }

    def __new__(_cls, path: PyPathBuf, config: &config) -> PyResult<Self> {
        let config = config.get_cfg(py);
        let abs_path = util::path::absolute(path.as_path()).map_pyerr(py)?;
        let repo = Repo::load_with_config(abs_path, config).map_pyerr(py)?;
        Self::create_instance(py, RwLock::new(repo))
    }

    def metalog(&self) -> PyResult<PyMetaLog> {
        let mut repo_ref = self.inner(py).write();
        let path = String::from(repo_ref.metalog_path().to_string_lossy());
        let log_ref = repo_ref.metalog().map_pyerr(py)?;
        PyMetaLog::create_instance(py, log_ref, path)
    }

    def invalidatemetalog(&self) -> PyResult<PyNone> {
        let mut repo_ref = self.inner(py).write();
        repo_ref.invalidate_metalog();
        Ok(PyNone)
    }

    def edenapi(&self) -> PyResult<PyEdenApi> {
        let mut repo_ref = self.inner(py).write();
        let edenapi_ref = repo_ref.eden_api().map_pyerr(py)?;
        PyEdenApi::create_instance(py, edenapi_ref)
    }

    def changelog(&self) -> PyResult<PyCommits> {
        let mut repo_ref = self.inner(py).write();
        let changelog_ref = py
            .allow_threads(|| repo_ref.dag_commits())
            .map_pyerr(py)?;
        PyCommits::create_instance(py, changelog_ref)
    }

    def invalidatechangelog(&self) -> PyResult<PyNone> {
        let mut repo_ref = self.inner(py).write();
        repo_ref.invalidate_dag_commits();
        Ok(PyNone)
    }
});
