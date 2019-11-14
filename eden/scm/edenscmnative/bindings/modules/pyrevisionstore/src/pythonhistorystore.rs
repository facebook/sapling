/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::{
    exc, FromPyObject, ObjectProtocol, PyBytes, PyClone, PyList, PyObject, PyTuple, Python,
    PythonObject, PythonObjectWithTypeObject,
};
use failure::Fallible as Result;

use cpython_ext::PyErr;
use revisionstore::{HistoryStore, LocalStore};
use types::{Key, NodeInfo};

use crate::pythonutil::{bytes_from_tuple, from_key_to_tuple, from_tuple_to_key, to_node_info};

pub struct PythonHistoryStore {
    py_store: PyObject,
}

impl PythonHistoryStore {
    pub fn new(py_store: PyObject) -> Self {
        PythonHistoryStore { py_store }
    }
}

impl HistoryStore for PythonHistoryStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let py_name = PyBytes::new(py, key.path.as_byte_slice());
        let py_node = PyBytes::new(py, key.node.as_ref());

        let py_data = match self.py_store.call_method(
            py,
            "getnodeinfo",
            (py_name.clone_ref(py), py_node),
            None,
        ) {
            Ok(data) => data,
            Err(py_err) => {
                if py_err.get_type(py) == exc::KeyError::type_object(py) {
                    return Ok(None);
                } else {
                    return Err(PyErr::from(py_err).into());
                }
            }
        };

        let py_tuple = PyTuple::extract(py, &py_data).map_err(|e| PyErr::from(e))?;

        let py_p1 = bytes_from_tuple(py, &py_tuple, 0)?;
        let py_p2 = bytes_from_tuple(py, &py_tuple, 1)?;
        let py_linknode = bytes_from_tuple(py, &py_tuple, 2)?;
        let py_copyfrom = py_tuple.get_item(py, 3);
        let py_copyfrom = Option::extract(py, &py_copyfrom).map_err(|e| PyErr::from(e))?;

        Ok(Some(
            to_node_info(py, &py_name, &py_p1, &py_p2, &py_linknode, py_copyfrom)
                .map_err(|e| PyErr::from(e))?,
        ))
    }
}

impl LocalStore for PythonHistoryStore {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let py_missing = PyList::new(py, &[]);
        for key in keys.iter() {
            let py_key = from_key_to_tuple(py, &key);
            py_missing.insert_item(py, py_missing.len(py), py_key.into_object());
        }

        let py_missing = self
            .py_store
            .call_method(py, "getmissing", (py_missing,), None)
            .map_err(|e| PyErr::from(e))?;
        let py_list = PyList::extract(py, &py_missing).map_err(|e| PyErr::from(e))?;
        let missing = py_list
            .iter(py)
            .map(|k| from_tuple_to_key(py, &k).map_err(|e| PyErr::from(e).into()))
            .collect::<Result<Vec<Key>>>()?;
        Ok(missing)
    }
}
