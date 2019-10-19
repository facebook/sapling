// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cpython::{
    PyBytes, PyErr, PyIterator, PyList, PyObject, PyResult, PyTuple, Python, PythonObject,
    ToPyObject,
};
use failure::Fallible;

use revisionstore::{HistoryStore, MutableHistoryStore, ToKeys};
use types::{Key, NodeInfo};

use crate::pythonutil::{
    from_key, from_key_to_tuple, from_tuple_to_key, key_error, to_key, to_node, to_path, to_pyerr,
};

pub trait HistoryStorePyExt {
    fn get_missing_py(&self, py: Python, keys: &mut PyIterator) -> PyResult<PyList>;
    fn get_node_info_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyTuple>;
}

pub trait IterableHistoryStorePyExt {
    fn iter_py(&self, py: Python) -> PyResult<Vec<PyTuple>>;
}

pub trait MutableHistoryStorePyExt: HistoryStorePyExt {
    fn add_py(
        &self,
        py: Python,
        name: &PyBytes,
        node: &PyBytes,
        p1: &PyBytes,
        p2: &PyBytes,
        linknode: &PyBytes,
        copyfrom: Option<&PyBytes>,
    ) -> PyResult<PyObject>;
    fn flush_py(&self, py: Python) -> PyResult<PyObject>;
}

impl<T: HistoryStore + ?Sized> HistoryStorePyExt for T {
    fn get_missing_py(&self, py: Python, keys: &mut PyIterator) -> PyResult<PyList> {
        // Copy the PyObjects into a vector so we can get a reference iterator.
        // This lets us get a Vector of Keys without copying the strings.
        let keys = keys
            .map(|k| k.and_then(|k| from_tuple_to_key(py, &k)))
            .collect::<Result<Vec<Key>, PyErr>>()?;
        let missing = self.get_missing(&keys[..]).map_err(|e| to_pyerr(py, &e))?;

        let results = PyList::new(py, &[]);
        for key in missing {
            let key_tuple = from_key_to_tuple(py, &key);
            results.insert_item(py, results.len(py), key_tuple.into_object());
        }

        Ok(results)
    }

    fn get_node_info_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyTuple> {
        let key = to_key(py, name, node)?;
        let info = self
            .get_node_info(&key)
            .map_err(|e| to_pyerr(py, &e))?
            .ok_or_else(|| key_error(py, &key))?;

        Ok(from_node_info(py, &key, &info))
    }
}

fn from_node_info(py: Python, key: &Key, info: &NodeInfo) -> PyTuple {
    (
        PyBytes::new(py, info.parents[0].hgid.as_ref()),
        PyBytes::new(py, info.parents[1].hgid.as_ref()),
        PyBytes::new(py, info.linknode.as_ref().as_ref()),
        if key.path != info.parents[0].path {
            if info.parents[0].path.is_empty() {
                Python::None(py)
            } else {
                PyBytes::new(py, info.parents[0].path.as_byte_slice()).into_object()
            }
        } else {
            Python::None(py)
        },
    )
        .into_py_object(py)
}

fn to_node_info(
    py: Python,
    name: &PyBytes,
    p1: &PyBytes,
    p2: &PyBytes,
    linknode: &PyBytes,
    copyfrom: Option<&PyBytes>,
) -> PyResult<NodeInfo> {
    // Not only can copyfrom be None, it can also be an empty string. In both cases that means that
    // `name` should be used.
    let copyfrom = copyfrom.unwrap_or(name);
    let p1path = if copyfrom.data(py).is_empty() {
        name
    } else {
        copyfrom
    };
    let p1node = to_node(py, p1);
    let p2node = to_node(py, p2);

    let parents = if p1node.is_null() {
        Default::default()
    } else if p2node.is_null() {
        let p1 = Key::new(to_path(py, p1path)?, p1node);
        let p2 = Key::default();
        [p1, p2]
    } else {
        let p1 = Key::new(to_path(py, p1path)?, p1node);
        let p2 = Key::new(to_path(py, name)?, p2node);
        [p1, p2]
    };

    let linknode = to_node(py, linknode);
    Ok(NodeInfo { parents, linknode })
}

impl<T: ToKeys + HistoryStore + ?Sized> IterableHistoryStorePyExt for T {
    fn iter_py(&self, py: Python) -> PyResult<Vec<PyTuple>> {
        let iter = self.to_keys().into_iter().map(|res| {
            let key = res?;
            let node_info = self.get_node_info(&key)?.unwrap();
            let (name, node) = from_key(py, &key);
            let copyfrom = if key.path != node_info.parents[0].path {
                if node_info.parents[0].path.is_empty() {
                    PyBytes::new(py, b"")
                } else {
                    PyBytes::new(py, node_info.parents[0].path.as_byte_slice())
                }
            } else {
                PyBytes::new(py, b"")
            };
            let tuple = (
                name.into_object(),
                node.into_object(),
                PyBytes::new(py, node_info.parents[0].hgid.as_ref()),
                PyBytes::new(py, node_info.parents[1].hgid.as_ref()),
                PyBytes::new(py, node_info.linknode.as_ref().as_ref()),
                copyfrom.into_object(),
            )
                .into_py_object(py);
            Ok(tuple)
        });
        iter.collect::<Fallible<Vec<PyTuple>>>()
            .map_err(|e| to_pyerr(py, &e.into()))
    }
}

impl<T: MutableHistoryStore + ?Sized> MutableHistoryStorePyExt for T {
    fn add_py(
        &self,
        py: Python,
        name: &PyBytes,
        node: &PyBytes,
        p1: &PyBytes,
        p2: &PyBytes,
        linknode: &PyBytes,
        copyfrom: Option<&PyBytes>,
    ) -> PyResult<PyObject> {
        let key = to_key(py, name, node)?;
        let nodeinfo = to_node_info(py, name, p1, p2, linknode, copyfrom)?;
        self.add(&key, &nodeinfo).map_err(|e| to_pyerr(py, &e))?;
        Ok(Python::None(py))
    }

    fn flush_py(&self, py: Python) -> PyResult<PyObject> {
        let opt = self.flush().map_err(|e| to_pyerr(py, &e))?;
        let opt = opt
            .as_ref()
            .map(|path| encoding::path_to_local_bytes(path))
            .transpose()
            .map_err(|e| to_pyerr(py, &e.into()))?;
        let opt = opt.map(|path| PyBytes::new(py, &path));
        Ok(opt.into_py_object(py))
    }
}
