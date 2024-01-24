/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::Arc;

use cpython::*;
use cpython_ext::convert::Serde;
use cpython_ext::PyNone;
use cpython_ext::ResultPyErrExt;
use edenfs_client::CheckoutConflict;
use edenfs_client::CheckoutMode;
use edenfs_client::FileStatus;
use types::HgId;
use types::RepoPathBuf;

py_class!(pub class EdenFsClient |py| {
    data inner: Arc<edenfs_client::EdenFsClient>;

    @property
    def root(&self) -> PyResult<String> {
        let inner = self.inner(py);
        Ok(inner.root().to_string())
    }

    /// get_status(commit, list_ignored=False) -> {path: 'A' | 'M' | 'R' | 'I'}
    def get_status(&self, commit: Serde<HgId>, list_ignored: bool = false) -> PyResult<Serde<BTreeMap<RepoPathBuf, FileStatus>>> {
        let inner = self.inner(py);
        let result = py.allow_threads(|| inner.get_status(commit.0, list_ignored)).map_pyerr(py)?;
        Ok(Serde(result))
    }

    /// set_parents(p1, p2, p1_tree) -> None
    def set_parents(&self, p1: Serde<HgId>, p2: Serde<Option<HgId>>, p1_tree: Serde<HgId>) -> PyResult<PyNone> {
        let inner = self.inner(py);
        py.allow_threads(|| inner.set_parents(p1.0, p2.0, p1_tree.0)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// checkout(node, tree_node, mode: 'NORMAL' | 'FORCE' | 'DRY_RUN')
    ///   -> [{'path': str, 'conflict_type': 'ERROR' | 'MODIFIED_REMOVED' | ..., 'message': str}]
    /// All conflict types: "ERROR", "MODIFIED_REMOVED", "UNTRACKED_ADDED", "REMOVED_MODIFIED",
    /// "MISSING_REMOVED", "MODIFIED_MODIFIED", "DIRECTORY_NOT_EMPTY".
    def checkout(&self, node: Serde<HgId>, tree_node: Serde<HgId>, mode: Serde<CheckoutMode>) -> PyResult<Serde<Vec<CheckoutConflict>>> {
        let inner = self.inner(py);
        let result = py.allow_threads(|| inner.checkout(node.0, tree_node.0, mode.0)).map_pyerr(py)?;
        Ok(Serde(result))
    }
});

py_exception!(error, EdenError);

pub(crate) fn populate_module(py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<EdenFsClient>(py)?;
    m.add(py, "EdenError", py.get_type::<EdenError>())?;
    cpython_ext::error::register("020-eden-error", eden_error_handler);
    Ok(())
}

fn eden_error_handler(py: Python, mut e: &cpython_ext::error::Error) -> Option<PyErr> {
    // Remove anyhow contex.
    while let Some(inner) = e.downcast_ref::<cpython_ext::error::Error>() {
        e = inner;
    }

    if let Some(e) = e.downcast_ref::<edenfs_client::EdenError>() {
        return Some(PyErr::new::<EdenError, _>(py, &e.message));
    }

    None
}
