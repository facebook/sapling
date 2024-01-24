/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

extern crate repo as rsrepo;
extern crate repolock as rsrepolock;
extern crate workingcopy as rsworkingcopy;

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use configmodel::config::ConfigExt;
use cpython::*;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::ExtractInner;
use cpython_ext::PyNone;
use cpython_ext::PyPathBuf;
use parking_lot::RwLock;
use pyconfigloader::config;
use pydag::commits::commits as PyCommits;
use pyeagerepo::EagerRepoStore as PyEagerRepoStore;
use pyedenapi::PyClient as PyEdenApi;
use pymetalog::metalog as PyMetaLog;
use pyrevisionstore::filescmstore as PyFileScmStore;
use pyrevisionstore::pyremotestore as PyRemoteStore;
use pyrevisionstore::treescmstore as PyTreeScmStore;
use pyworkingcopy::workingcopy as PyWorkingCopy;
use revisionstore::ContentStoreBuilder;
use rsrepo::repo::Repo;
use rsworkingcopy::workingcopy::WorkingCopy;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "repo"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<repo>(py)?;
    m.add_class::<repolock>(py)?;
    Ok(m)
}

py_class!(pub class repo |py| {
    data inner: RwLock<Repo>;
    data inner_wc: RefCell<Option<Arc<RwLock<WorkingCopy>>>>;

    @staticmethod
    def initialize(path: PyPathBuf, config: &config, repo_config: Option<String>) -> PyResult<PyNone> {
        Repo::init(path.as_path(), &config.get_cfg(py), repo_config, &[]).map_pyerr(py)?;
        Ok(PyNone)
    }

    def __new__(_cls, path: PyPathBuf, config: &config) -> PyResult<Self> {
        let config = config.get_cfg(py);
        let abs_path = util::path::absolute(path.as_path()).map_pyerr(py)?;
        let repo = Repo::load_with_config(abs_path, config).map_pyerr(py)?;
        Self::create_instance(py, RwLock::new(repo), RefCell::new(None))
    }

    def workingcopy(&self) -> PyResult<PyWorkingCopy> {
        let mut wc_option = self.inner_wc(py).borrow_mut();
        if wc_option.is_none() {
            let mut repo = self.inner(py).write();
            wc_option.replace(Arc::new(RwLock::new(repo.working_copy().map_pyerr(py)?)));
        }
        PyWorkingCopy::create_instance(py, wc_option.as_ref().unwrap().clone())
    }

    def invalidateworkingcopy(&self) -> PyResult<PyNone> {
        let wc_option = self.inner_wc(py).borrow_mut();
        if wc_option.is_some() {
            let mut repo = self.inner(py).write();
            let mut wc = wc_option.as_ref().unwrap().write();
            *wc = repo.working_copy().map_pyerr(py)?;
        }
        Ok(PyNone)
    }

    def metalog(&self) -> PyResult<PyMetaLog> {
        let mut repo_ref = self.inner(py).write();
        let path = String::from(repo_ref.metalog_path().to_string_lossy());
        let log_ref = repo_ref.metalog().map_pyerr(py)?;
        PyMetaLog::create_instance(py, log_ref, path)
    }

    @property
    def requirements(&self) -> PyResult<HashSet<String>> {
        let repo_ref = self.inner(py).read();
        Ok(repo_ref.requirements.to_set())
    }

    @property
    def store_requirements(&self) -> PyResult<HashSet<String>> {
        let repo_ref = self.inner(py).read();
        Ok(repo_ref.store_requirements.to_set())
    }

    @property
    def storage_format(&self) -> PyResult<String> {
        let repo_ref = self.inner(py).read();
        let format = repo_ref.storage_format();
        let lower_case = format!("{:?}", format).to_lowercase();
        Ok(lower_case)
    }

    def invalidatemetalog(&self) -> PyResult<PyNone> {
        let mut repo_ref = self.inner(py).write();
        repo_ref.invalidate_metalog();
        Ok(PyNone)
    }

    def edenapi(&self) -> PyResult<PyEdenApi> {
        let repo_ref = self.inner(py).read();
        let edenapi_ref = repo_ref.eden_api().map_pyerr(py)?;
        PyEdenApi::create_instance(py, edenapi_ref)
    }

    def nullableedenapi(&self) -> PyResult<Option<PyEdenApi>> {
        let repo_ref = self.inner(py).read();
        match repo_ref.optional_eden_api().map_pyerr(py)? {
            Some(api) => Ok(Some(PyEdenApi::create_instance(py, api)?)),
            None => Ok(None),
        }
    }

    def filescmstore(&self, remote: PyRemoteStore) -> PyResult<PyFileScmStore> {
        let mut repo = self.inner(py).write();
        let _ = repo.file_store().map_pyerr(py)?;
        let mut file_scm_store = repo.file_scm_store().unwrap();

        let mut builder = ContentStoreBuilder::new(repo.config())
            .remotestore(remote.extract_inner(py))
            .local_path(repo.store_path());

        if let Some(indexedlog_local) = file_scm_store.indexedlog_local() {
            builder = builder.shared_indexedlog_local(indexedlog_local);
        }

        if let Some(cache) = file_scm_store.indexedlog_cache() {
            builder = builder.shared_indexedlog_shared(cache);
        }

        let contentstore = Arc::new(builder.build().map_pyerr(py)?);

        if repo.config().get_or_default("scmstore", "contentstorefallback").map_pyerr(py)? {
            file_scm_store = Arc::new(file_scm_store.with_content_store(contentstore.clone()));
        }

        PyFileScmStore::create_instance(py, file_scm_store, contentstore)
    }

    def treescmstore(&self, remote: PyRemoteStore) -> PyResult<PyTreeScmStore> {
        let mut repo = self.inner(py).write();
        let _ = repo.tree_store().map_pyerr(py)?;
        let mut tree_scm_store = repo.tree_scm_store().unwrap();

        let mut builder = ContentStoreBuilder::new(repo.config())
            .remotestore(remote.extract_inner(py))
            .local_path(repo.store_path())
            .suffix("manifests");

        if let Some(indexedlog_local) = tree_scm_store.indexedlog_local.clone() {
            builder = builder.shared_indexedlog_local(indexedlog_local);
        }

        if let Some(cache) = tree_scm_store.indexedlog_cache.clone() {
            builder = builder.shared_indexedlog_shared(cache);
        }

        let contentstore = Arc::new(builder.build().map_pyerr(py)?);

        if repo.config().get_or_default("scmstore", "contentstorefallback").map_pyerr(py)? {
            tree_scm_store = Arc::new(tree_scm_store.with_content_store(contentstore.clone()));
        }


        PyTreeScmStore::create_instance(py, tree_scm_store, contentstore)
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
        repo_ref.invalidate_dag_commits().map_pyerr(py)?;
        Ok(PyNone)
    }

    def invalidatestores(&self) -> PyResult<PyNone> {
        let mut repo_ref = self.inner(py).write();
        repo_ref.invalidate_stores().map_pyerr(py)?;
        Ok(PyNone)
    }

    def invalidaterequires(&self) -> PyResult<PyNone> {
        let mut repo_ref = self.inner(py).write();
        repo_ref.reload_requires().map_pyerr(py)?;
        Ok(PyNone)
    }

    def dotpath(&self) -> PyResult<PyPathBuf> {
        self.inner(py).read().dot_hg_path().try_into().map_pyerr(py)
    }

    def trywlock(&self, wc_dot_hg: PyPathBuf) -> PyResult<repolock> {
        let lock = self.inner(py).read().locker().try_lock_working_copy(wc_dot_hg.to_path_buf()).map_pyerr(py)?;
        if lock.count() > 1 {
            Err(PyErr::new::<exc::ValueError, _>(py, "lock is already locked"))
        } else {
            repolock::create_instance(py, Cell::new(Some(lock)))
        }
    }

    def trylock(&self) -> PyResult<repolock> {
        let lock = self.inner(py).read().locker().try_lock_store().map_pyerr(py)?;
        if lock.count() > 1 {
            Err(PyErr::new::<exc::ValueError, _>(py, "lock is already locked"))
        } else {
            repolock::create_instance(py, Cell::new(Some(lock)))
        }
    }

    def eagerstore(&self) -> PyResult<PyEagerRepoStore> {
        let mut repo = self.inner(py).write();
        let _ = repo.file_store().map_pyerr(py)?;
        PyEagerRepoStore::create_instance(py, repo.eager_store().unwrap())
    }
});

py_class!(pub class repolock |py| {
    data lock: Cell<Option<rsrepolock::LockedPath>>;

    def unlock(&self) -> PyResult<PyNone> {
        if let Some(f) = self.lock(py).replace(None) {
            let count = f.count();
            drop(f);
            if count == 1 {
                Ok(PyNone)
            } else {
                Err(PyErr::new::<exc::ValueError, _>(py, "lock is still locked"))
            }
        } else {
            Err(PyErr::new::<exc::ValueError, _>(py, "lock is already unlocked"))
        }
    }
});
