// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io;

use cpython::{
    exc, FromPyObject, PyBytes, PyDict, PyErr, PyObject, PyResult, PyTuple, Python, PythonObject,
    ToPyObject,
};
use failure::{Error, Fallible};

use revisionstore::datastore::{Delta, Metadata};
use types::{Key, Node, RepoPath, RepoPathBuf};

use crate::pyerror::PythonError;

pub fn to_pyerr(py: Python, error: &Error) -> PyErr {
    if let Some(io_error) = error.downcast_ref::<io::Error>() {
        PyErr::new::<exc::OSError, _>(
            py,
            (io_error.raw_os_error(), format!("{}", error.as_fail())),
        )
    } else {
        PyErr::new::<exc::RuntimeError, _>(py, format!("{}", error.as_fail()))
    }
}

pub fn to_node(py: Python, node: &PyBytes) -> Node {
    let mut bytes: [u8; 20] = Default::default();
    bytes.copy_from_slice(&node.data(py)[0..20]);
    (&bytes).into()
}

pub fn to_path(py: Python, name: &PyBytes) -> PyResult<RepoPathBuf> {
    RepoPath::from_utf8(name.data(py))
        .map_err(|e| to_pyerr(py, &e))
        .map(|path| path.to_owned())
}

pub fn to_key(py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<Key> {
    let node = to_node(py, node);
    let path = to_path(py, name)?;
    Ok(Key::new(path, node))
}

pub fn from_key(py: Python, key: &Key) -> (PyBytes, PyBytes) {
    (
        PyBytes::new(py, key.path.as_byte_slice()),
        PyBytes::new(py, key.node.as_ref()),
    )
}

pub fn to_delta(
    py: Python,
    name: &PyBytes,
    node: &PyBytes,
    deltabasenode: &PyBytes,
    data: &PyBytes,
) -> PyResult<Delta> {
    let key = to_key(py, name, node)?;
    let base_key = to_key(py, name, deltabasenode)?;
    Ok(Delta {
        data: data.data(py).to_vec().into(),
        base: if base_key.node.is_null() {
            None
        } else {
            Some(base_key)
        },
        key,
    })
}

pub fn from_tuple_to_delta<'a>(py: Python, py_delta: &PyObject) -> PyResult<Delta> {
    // A python delta is a tuple: (name, node, base name, base node, delta bytes)
    let py_delta = PyTuple::extract(py, &py_delta)?;
    let py_name = PyBytes::extract(py, &py_delta.get_item(py, 0))?;
    let py_node = PyBytes::extract(py, &py_delta.get_item(py, 1))?;
    let py_delta_name = PyBytes::extract(py, &py_delta.get_item(py, 2))?;
    let py_delta_node = PyBytes::extract(py, &py_delta.get_item(py, 3))?;
    let py_bytes = PyBytes::extract(py, &py_delta.get_item(py, 4))?;

    let key = to_key(py, &py_name, &py_node)?;
    let base_key = to_key(py, &py_delta_name, &py_delta_node)?;
    Ok(Delta {
        data: py_bytes.data(py).to_vec().into(),
        base: if base_key.node.is_null() {
            None
        } else {
            Some(base_key)
        },
        key,
    })
}

pub fn from_base(py: Python, delta: &Delta) -> (PyBytes, PyBytes) {
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
        name.into_object(),
        node.into_object(),
        base_name.into_object(),
        base_node.into_object(),
        bytes.into_object(),
    )
        .into_py_object(py)
        .into_object()
}

pub fn from_key_to_tuple<'a>(py: Python, key: &'a Key) -> PyTuple {
    let (py_name, py_node) = from_key(py, key);
    PyTuple::new(py, &[py_name.into_object(), py_node.into_object()])
}

pub fn from_tuple_to_key(py: Python, py_tuple: &PyObject) -> PyResult<Key> {
    let py_tuple = <&PyTuple>::extract(py, &py_tuple)?.as_slice(py);
    let name = <&PyBytes>::extract(py, &py_tuple[0])?;
    let node = <&PyBytes>::extract(py, &py_tuple[1])?;
    to_key(py, &name, &node)
}

pub fn bytes_from_tuple(py: Python, tuple: &PyTuple, index: usize) -> Fallible<PyBytes> {
    PyBytes::extract(py, &tuple.get_item(py, index)).map_err(|e| PythonError::from(e).into())
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

pub fn key_error(py: Python, key: &Key) -> PyErr {
    PyErr::new::<exc::KeyError, _>(py, format!("Key not found {:?}", key))
}
