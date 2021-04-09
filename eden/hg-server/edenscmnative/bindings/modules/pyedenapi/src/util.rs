/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cpython::*;

use cpython_ext::{ExtractInner, PyPath, PyPathBuf, ResultPyErrExt};
use edenapi::{Progress, ProgressCallback, ResponseMeta};
use edenapi_types::TreeAttributes;
use pyrevisionstore::{mutabledeltastore, mutablehistorystore};
use revisionstore::{HgIdMutableDeltaStore, HgIdMutableHistoryStore};
use types::{HgId, Key, RepoPathBuf};

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

pub fn to_tree_attrs(py: Python, attrs: &PyDict) -> PyResult<TreeAttributes> {
    let mut attributes = TreeAttributes::default();

    attributes.manifest_blob = attrs
        .get_item(py, "manifest_blob")
        .map(|v| v.extract::<bool>(py))
        .transpose()?
        .unwrap_or(attributes.manifest_blob);
    attributes.parents = attrs
        .get_item(py, "parents")
        .map(|v| v.extract::<bool>(py))
        .transpose()?
        .unwrap_or(attributes.parents);
    attributes.child_metadata = attrs
        .get_item(py, "child_metadata")
        .map(|v| v.extract::<bool>(py))
        .transpose()?
        .unwrap_or(attributes.child_metadata);

    Ok(attributes)
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

pub fn meta_to_dict(py: Python, meta: &ResponseMeta) -> PyResult<PyDict> {
    let dict = PyDict::new(py);
    dict.set_item(py, "version", format!("{:?}", &meta.version))?;
    dict.set_item(py, "status", meta.status.as_u16())?;
    dict.set_item(py, "server", &meta.server)?;
    dict.set_item(py, "request_id", &meta.request_id)?;
    dict.set_item(py, "tw_task_handle", &meta.tw_task_handle)?;
    dict.set_item(py, "tw_task_version", &meta.tw_task_version)?;
    dict.set_item(py, "tw_canary_id", &meta.tw_canary_id)?;
    dict.set_item(py, "server_load", &meta.server_load)?;
    dict.set_item(py, "content_length", &meta.content_length)?;
    Ok(dict)
}
