/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use anyhow::Result;
use async_runtime::block_on_exclusive as block_on;
use checkout::CheckoutPlan;
use cpython::*;
use cpython_ext::{ExtractInnerRef, PyNone, PyPathBuf, ResultPyErrExt};
use manifest_tree::Diff;
use pathmatcher::{AlwaysMatcher, Matcher};
use pymanifest::treemanifest;
use pypathmatcher::PythonMatcher;
use pyrevisionstore::contentstore;
use pytreestate::treestate as PyTreeState;
use std::time::SystemTime;
use tracing::warn;
use treestate::filestate::{FileStateV2, StateFlags};
use types::RepoPath;
use vfs::VFS;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "checkout"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<checkoutplan>(py)?;
    Ok(m)
}

py_class!(class checkoutplan |py| {
    data plan: CheckoutPlan;

    def __new__(
        _cls,
        current_manifest: &treemanifest,
        target_manifest: &treemanifest,
        matcher: Option<PyObject> = None,
    ) -> PyResult<checkoutplan> {
        let matcher: Box<dyn Matcher> = match matcher {
            None => Box::new(AlwaysMatcher::new()),
            Some(pyobj) => Box::new(PythonMatcher::new(py, pyobj)),
        };

        let current = current_manifest.borrow_underlying(py);
        let target = target_manifest.borrow_underlying(py);
        let diff = Diff::new(&current, &target, &matcher);
        let plan = CheckoutPlan::from_diff(diff).map_pyerr(py)?;
        checkoutplan::create_instance(py, plan)
    }

    def apply(&self, root: PyPathBuf, content_store: &contentstore) -> PyResult<PyNone> {
        let vfs = VFS::new(root.to_path_buf()).map_pyerr(py)?;
        let store = content_store.extract_inner_ref(py);
        let plan = self.plan(py);
        block_on(plan.apply_remote_data_store(&vfs, store)).map_pyerr(py)?;
        Ok(PyNone)
    }

    def stats(&self) -> PyResult<(usize, usize, usize, usize)> {
        let plan = self.plan(py);
        let (updated, removed) = plan.stats();
        let (merged, unresolved) = (0, 0);

        Ok((updated, merged, removed, unresolved))
    }

    def record_updates(&self, root: PyPathBuf, state: &PyTreeState) -> PyResult<PyNone> {
        let vfs = VFS::new(root.to_path_buf()).map_pyerr(py)?;
        let plan = self.plan(py);
        let state = state.get_state(py);
        let mut state = state.lock();

        for removed in plan.removed_files() {
            state.remove(removed).map_pyerr(py)?;
        }

        for updated in plan.updated_content_files().chain(plan.updated_meta_files()) {
            let fstate = file_state(&vfs, updated).map_pyerr(py)?;
            state.insert(updated, &fstate).map_pyerr(py)?;
        }

        Ok(PyNone)
    }

    def __str__(&self) -> PyResult<PyString> {
        Ok(PyString::new(py, &self.plan(py).to_string()))
    }

});

fn file_state(vfs: &VFS, path: &RepoPath) -> Result<FileStateV2> {
    let meta = vfs.metadata(path)?;
    #[cfg(unix)]
    let mode = std::os::unix::fs::PermissionsExt::mode(&meta.permissions());
    #[cfg(windows)]
    let mode = 0o644; // todo figure this out
    let mtime = meta
        .modified()?
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs();
    let mtime = truncate_u64("mtime", path, mtime);
    let size = meta.len();
    let size = truncate_u64("size", path, size);
    let state = StateFlags::EXIST_P1 | StateFlags::EXIST_NEXT;
    Ok(FileStateV2 {
        mode,
        size,
        mtime,
        state,
        copied: None,
    })
}

fn truncate_u64(f: &str, path: &RepoPath, v: u64) -> i32 {
    const RANGE_MASK: u64 = 0x7FFFFFFF;
    let truncated = v & RANGE_MASK;
    if truncated != v {
        warn!("{} for {} is truncated {}=>{}", f, path, v, truncated);
    }
    truncated as i32
}
