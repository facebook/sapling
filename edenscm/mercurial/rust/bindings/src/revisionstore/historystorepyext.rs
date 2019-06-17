// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cpython::{
    PyBytes, PyDict, PyErr, PyIterator, PyList, PyResult, PyTuple, Python, PythonObject, ToPyObject,
};

use revisionstore::historystore::HistoryStore;
use types::{Key, NodeInfo};

use crate::revisionstore::pythonutil::{from_key_to_tuple, from_tuple_to_key, to_key, to_pyerr};

pub trait HistoryStorePyExt {
    fn get_ancestors_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict>;
    fn get_missing_py(&self, py: Python, keys: &mut PyIterator) -> PyResult<PyList>;
    fn get_node_info_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyTuple>;
}

impl<T: HistoryStore + ?Sized> HistoryStorePyExt for T {
    fn get_ancestors_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict> {
        let key = to_key(py, name, node)?;
        let ancestors = self.get_ancestors(&key).map_err(|e| to_pyerr(py, &e))?;
        let ancestors = ancestors
            .iter()
            .map(|(k, v)| (PyBytes::new(py, k.node.as_ref()), from_node_info(py, k, v)));
        let pyancestors = PyDict::new(py);
        for (node, value) in ancestors {
            pyancestors.set_item(py, node, value)?;
        }
        Ok(pyancestors)
    }

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
        let info = self.get_node_info(&key).map_err(|e| to_pyerr(py, &e))?;
        Ok(from_node_info(py, &key, &info))
    }
}

fn from_node_info(py: Python, key: &Key, info: &NodeInfo) -> PyTuple {
    (
        PyBytes::new(py, info.parents[0].node.as_ref()),
        PyBytes::new(py, info.parents[1].node.as_ref()),
        PyBytes::new(py, info.linknode.as_ref().as_ref()),
        if key.path != info.parents[0].path {
            PyBytes::new(py, info.parents[0].path.as_byte_slice()).into_object()
        } else {
            Python::None(py)
        },
    )
        .into_py_object(py)
}
