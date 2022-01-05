/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cpython::exc;
use cpython::FromPyObject;
use cpython::PyBytes;
use cpython::PyClone;
use cpython::PyDict;
use cpython::PyErr;
use cpython::PyObject;
use cpython::PyResult;
use cpython::PyTuple;
use cpython::Python;
use cpython::PythonObject;
use cpython::PythonObjectWithCheckedDowncast;
use cpython::ToPyObject;
use cpython_ext::ExtractInner;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use revisionstore::datastore::Delta;
use revisionstore::datastore::Metadata;
use revisionstore::LegacyStore;
use revisionstore::StoreKey;
use types::Key;
use types::Node;
use types::RepoPathBuf;

use crate::contentstore;
use crate::filescmstore;
use crate::treescmstore;

pub fn to_node(py: Python, node: &PyBytes) -> Node {
    let mut bytes: [u8; 20] = Default::default();
    bytes.copy_from_slice(&node.data(py)[0..20]);
    (&bytes).into()
}

pub fn to_path(py: Python, name: &PyPath) -> PyResult<RepoPathBuf> {
    name.to_repo_path()
        .map_pyerr(py)
        .map(|path| path.to_owned())
}

pub fn to_key(py: Python, name: &PyPath, node: &PyBytes) -> PyResult<Key> {
    let node = to_node(py, node);
    let path = to_path(py, name)?;
    Ok(Key::new(path, node))
}

pub fn from_key(py: Python, key: &Key) -> (PyPathBuf, PyBytes) {
    (
        PyPathBuf::from(key.path.as_repo_path()),
        PyBytes::new(py, key.hgid.as_ref()),
    )
}

pub fn to_delta(
    py: Python,
    name: &PyPath,
    node: &PyBytes,
    deltabasenode: &PyBytes,
    data: &PyBytes,
) -> PyResult<Delta> {
    let key = to_key(py, name, node)?;
    let base_key = to_key(py, name, deltabasenode)?;
    Ok(Delta {
        data: data.data(py).to_vec().into(),
        base: if base_key.hgid.is_null() {
            None
        } else {
            Some(base_key)
        },
        key,
    })
}

pub fn as_legacystore(py: Python, store: PyObject) -> PyResult<Arc<dyn LegacyStore>> {
    Ok(contentstore::downcast_from(py, store.clone_ref(py))
        .map(|s| s.extract_inner(py) as Arc<dyn LegacyStore>)
        .or_else(|_| {
            filescmstore::downcast_from(py, store.clone_ref(py))
                .map(|s| s.extract_inner(py) as Arc<dyn LegacyStore>)
        })
        .or_else(|_| {
            treescmstore::downcast_from(py, store)
                .map(|s| s.extract_inner(py) as Arc<dyn LegacyStore>)
        })?)
}

pub fn from_base(py: Python, delta: &Delta) -> (PyPathBuf, PyBytes) {
    match delta.base.as_ref() {
        Some(base) => from_key(py, &base),
        None => from_key(
            py,
            &Key::new(delta.key.path.clone(), Node::null_id().clone()),
        ),
    }
}

pub fn from_delta_to_tuple(py: Python, delta: &Delta) -> PyObject {
    let (name, node) = from_key(py, &delta.key);
    let (base_name, base_node) = from_base(py, &delta);
    let bytes = PyBytes::new(py, &delta.data);
    // A python delta is a tuple: (name, node, base name, base node, delta bytes)
    (
        name.to_py_object(py).into_object(),
        node.into_object(),
        base_name.to_py_object(py).into_object(),
        base_node.into_object(),
        bytes.into_object(),
    )
        .into_py_object(py)
        .into_object()
}

pub fn from_key_to_tuple<'a>(py: Python, key: &'a Key) -> PyTuple {
    let (py_name, py_node) = from_key(py, key);
    PyTuple::new(
        py,
        &[
            py_name.to_py_object(py).into_object(),
            py_node.into_object(),
        ],
    )
}

pub fn from_tuple_to_key(py: Python, py_tuple: &PyObject) -> PyResult<Key> {
    let py_tuple = <&PyTuple>::extract(py, &py_tuple)?.as_slice(py);
    let name = <PyPathBuf>::extract(py, &py_tuple[0])?;
    let node = <&PyBytes>::extract(py, &py_tuple[1])?;
    to_key(py, &name, &node)
}

pub fn to_metadata(py: Python, meta: &PyDict) -> PyResult<Metadata> {
    Ok(Metadata {
        flags: match meta.get_item(py, "f") {
            Some(x) => Some(u64::extract(py, &x)?),
            None => None,
        },
        size: match meta.get_item(py, "s") {
            Some(x) => Some(u64::extract(py, &x)?),
            None => None,
        },
    })
}

pub fn key_error(py: Python, key: &StoreKey) -> PyErr {
    PyErr::new::<exc::KeyError, _>(py, format!("Key not found {:?}", key))
}
