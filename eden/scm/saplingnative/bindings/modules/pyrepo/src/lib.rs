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

use checkout::BookmarkAction;
use checkout::CheckoutMode;
use checkout::ReportMode;
use context::CoreContext;
use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::convert::Serde;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::PyNone;
use cpython_ext::PyPathBuf;
use parking_lot::RwLock;
use pyconfigloader::config;
use pydag::commits::commits as PyCommits;
use pyeagerepo::EagerRepoStore as PyEagerRepoStore;
use pyedenapi::PyClient as PySaplingRemoteApi;
use pymetalog::metalog as PyMetaLog;
use pyrevisionstore::filescmstore as PyFileScmStore;
use pyrevisionstore::treescmstore as PyTreeScmStore;
use pyworkingcopy::workingcopy as PyWorkingCopy;
use rsrepo::repo::Repo;
use rsworkingcopy::workingcopy::WorkingCopy;
use types::HgId;

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
            let repo = self.inner(py).write();
            wc_option.replace(repo.working_copy().map_pyerr(py)?);
        }
        PyWorkingCopy::create_instance(py, wc_option.as_ref().unwrap().clone())
    }

    def invalidateworkingcopy(&self) -> PyResult<PyNone> {
        let wc_option = self.inner_wc(py).borrow_mut();
        if wc_option.is_some() {
            let repo = self.inner(py).write();
            repo.invalidate_working_copy().map_pyerr(py)?;
        }
        Ok(PyNone)
    }

    def metalog(&self) -> PyResult<PyMetaLog> {
        let repo_ref = self.inner(py).write();
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
        let repo_ref = self.inner(py).write();
        repo_ref.invalidate_metalog().map_pyerr(py)?;
        Ok(PyNone)
    }

    def edenapi(&self) -> PyResult<PySaplingRemoteApi> {
        let repo_ref = self.inner(py).read();
        let edenapi_ref = repo_ref.eden_api().map_pyerr(py)?;
        PySaplingRemoteApi::create_instance(py, edenapi_ref)
    }

    def nullableedenapi(&self) -> PyResult<Option<PySaplingRemoteApi>> {
        let repo_ref = self.inner(py).read();
        match repo_ref.optional_eden_api().map_pyerr(py)? {
            Some(api) => Ok(Some(PySaplingRemoteApi::create_instance(py, api)?)),
            None => Ok(None),
        }
    }

    def filescmstore(&self) -> PyResult<PyFileScmStore> {
        let repo = self.inner(py).write();
        let _ = repo.file_store().map_pyerr(py)?;
        let file_scm_store = repo.file_scm_store().unwrap();

        PyFileScmStore::create_instance(py, file_scm_store)
    }

    def treescmstore(&self) -> PyResult<PyTreeScmStore> {
        let repo = self.inner(py).write();
        let _ = repo.tree_store().map_pyerr(py)?;
        let tree_scm_store = repo.tree_scm_store().unwrap();

        let caching_store = Some(repo.caching_tree_store().map_pyerr(py)?);

        PyTreeScmStore::create_instance(py, tree_scm_store, caching_store)
    }

    def changelog(&self) -> PyResult<PyCommits> {
        let repo_ref = self.inner(py).write();
        let changelog_ref = py
            .allow_threads(|| repo_ref.dag_commits())
            .map_pyerr(py)?;
        PyCommits::create_instance(py, changelog_ref)
    }

    def invalidatechangelog(&self) -> PyResult<PyNone> {
        let repo_ref = self.inner(py).write();
        repo_ref.invalidate_dag_commits().map_pyerr(py)?;
        Ok(PyNone)
    }

    def invalidatestores(&self) -> PyResult<PyNone> {
        let repo_ref = self.inner(py).write();
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
        let repo = self.inner(py).write();
        let _ = repo.file_store().map_pyerr(py)?;
        PyEagerRepoStore::create_instance(py, repo.eager_store().unwrap())
    }

    def goto(
        &self,
        ctx: ImplInto<CoreContext>,
        target: Serde<HgId>,
        bookmark: Serde<BookmarkAction>,
        mode: Serde<CheckoutMode>,
        report_mode: Serde<ReportMode>,
    ) -> PyResult<(usize, usize, usize, usize)> {
        let repo = self.inner(py).read();
        let wc = self.workingcopy(py)?.get_wc(py);
        let wc = wc.write();
        let flush_dirstate = !wc.is_locked();
        checkout::checkout(
            &ctx.0,
            &repo,
            &wc.lock().map_pyerr(py)?,
            target.0,
            bookmark.0,
            mode.0,
            report_mode.0,
            flush_dirstate,
        ).map(|opt_stats| {
            let (updated, removed) = opt_stats.unwrap_or_default();
            (updated, 0, removed, 0)
        }).map_pyerr(py)
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
