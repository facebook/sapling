/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cpython::*;

use cpython_ext::{ExtractInner, PyPath, PyPathBuf, ResultPyErrExt};
use edenapi::{Progress, ProgressCallback, RepoName};
use pyrevisionstore::{mutabledeltastore, mutablehistorystore};
use revisionstore::{HgIdMutableDeltaStore, HgIdMutableHistoryStore};
use types::{HgId, Key, RepoPathBuf};

pub fn to_repo_name(py: Python, repo: &str) -> PyResult<RepoName> {
    repo.parse().map_pyerr(py)
}

pub fn to_path(py: Python, name: &PyPath) -> PyResult<RepoPathBuf> {
    name.to_repo_path()
        .map_pyerr(py)
        .map(|path| path.to_owned())
}

pub fn to_hgid(py: Python, hgid: &PyBytes) -> HgId {
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(&hgid.data(py)[0..20]);
    HgId::from(&bytes)
}

pub fn to_hgids(py: Python, hgids: impl IntoIterator<Item = PyBytes>) -> Vec<HgId> {
    hgids.into_iter().map(|hgid| to_hgid(py, &hgid)).collect()
}

pub fn to_key(py: Python, path: &PyPath, hgid: &PyBytes) -> PyResult<Key> {
    let hgid = to_hgid(py, hgid);
    let path = to_path(py, path)?;
    Ok(Key::new(path, hgid))
}

pub fn to_keys<'a>(
    py: Python,
    keys: impl IntoIterator<Item = &'a (PyPathBuf, PyBytes)>,
) -> PyResult<Vec<Key>> {
    keys.into_iter()
        .map(|(path, hgid)| to_key(py, path, hgid))
        .collect()
}

pub fn wrap_callback(callback: PyObject) -> ProgressCallback {
    Box::new(move |progress: Progress| {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let _ = callback.call(py, progress.as_tuple(), None);
    })
}

pub fn as_deltastore(py: Python, store: PyObject) -> PyResult<Arc<dyn HgIdMutableDeltaStore>> {
    Ok(store.extract::<mutabledeltastore>(py)?.extract_inner(py))
}

pub fn as_historystore(py: Python, store: PyObject) -> PyResult<Arc<dyn HgIdMutableHistoryStore>> {
    Ok(store.extract::<mutablehistorystore>(py)?.extract_inner(py))
}
