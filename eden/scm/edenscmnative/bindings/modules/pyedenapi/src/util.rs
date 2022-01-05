/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cpython::*;
use cpython_ext::convert::Serde;
use cpython_ext::ExtractInner;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use edenapi::ResponseMeta;
use edenapi_types::ContentId;
use edenapi_types::UploadTreeEntry;
use pyrevisionstore::mutabledeltastore;
use revisionstore::HgIdMutableDeltaStore;
use types::HgId;
use types::Key;
use types::Parents;
use types::RepoPathBuf;

pub fn to_path(py: Python, name: &PyPath) -> PyResult<RepoPathBuf> {
    name.to_repo_path()
        .map_pyerr(py)
        .map(|path| path.to_owned())
}

pub fn to_contentid(py: Python, content_id: &PyBytes) -> ContentId {
    let mut bytes = [0u8; ContentId::len()];
    bytes.copy_from_slice(&content_id.data(py)[0..ContentId::len()]);
    ContentId::from(bytes)
}

pub fn to_key(py: Python, path: &PyPath, hgid: HgId) -> PyResult<Key> {
    let path = to_path(py, path)?;
    Ok(Key::new(path, hgid))
}

pub fn to_key_with_parents(
    py: Python,
    path: &PyPath,
    hgid: HgId,
    p1: HgId,
    p2: HgId,
) -> PyResult<(Key, Parents)> {
    let path = to_path(py, path)?;
    Ok((Key::new(path, hgid), Parents::new(p1, p2)))
}

pub fn to_trees_upload_item(
    py: Python,
    hgid: HgId,
    p1: HgId,
    p2: HgId,
    data: &PyBytes,
) -> PyResult<UploadTreeEntry> {
    Ok(UploadTreeEntry {
        node_id: hgid,
        data: data.data(py).to_vec(),
        parents: Parents::new(p1, p2),
    })
}

pub fn to_keys<'a>(
    py: Python,
    keys: impl IntoIterator<Item = &'a (PyPathBuf, Serde<HgId>)>,
) -> PyResult<Vec<Key>> {
    keys.into_iter()
        .map(|(path, hgid)| to_key(py, path, hgid.0))
        .collect()
}

pub fn to_keys_with_parents<'a>(
    py: Python,
    keys: impl IntoIterator<Item = &'a (PyPathBuf, Serde<HgId>, Serde<HgId>, Serde<HgId>)>,
) -> PyResult<Vec<(Key, Parents)>> {
    keys.into_iter()
        .map(|(path, hgid, p1, p2)| to_key_with_parents(py, path, hgid.0, p1.0, p2.0))
        .collect()
}

pub fn to_trees_upload_items<'a>(
    py: Python,
    items: impl IntoIterator<Item = &'a (Serde<HgId>, Serde<HgId>, Serde<HgId>, PyBytes)>,
) -> PyResult<Vec<UploadTreeEntry>> {
    items
        .into_iter()
        .map(|(hgid, p1, p2, data)| to_trees_upload_item(py, hgid.0, p1.0, p2.0, data))
        .collect()
}

pub fn as_deltastore(py: Python, store: PyObject) -> PyResult<Arc<dyn HgIdMutableDeltaStore>> {
    Ok(store.extract::<mutabledeltastore>(py)?.extract_inner(py))
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
    dict.set_item(py, "content_encoding", &meta.content_encoding)?;
    Ok(dict)
}
