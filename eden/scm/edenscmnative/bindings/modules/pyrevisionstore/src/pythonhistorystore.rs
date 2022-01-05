/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use cpython::exc;
use cpython::FromPyObject;
use cpython::NoArgs;
use cpython::ObjectProtocol;
use cpython::PyBytes;
use cpython::PyDict;
use cpython::PyList;
use cpython::PyObject;
use cpython::Python;
use cpython::PythonObject;
use cpython::PythonObjectWithTypeObject;
use cpython_ext::PyErr;
use cpython_ext::PyPathBuf;
use revisionstore::HgIdHistoryStore;
use revisionstore::LocalStore;
use revisionstore::StoreKey;
use types::Key;
use types::NodeInfo;

use crate::pythonutil::bytes_from_tuple;
use crate::pythonutil::from_key_to_tuple;
use crate::pythonutil::from_tuple_to_key;
use crate::pythonutil::to_node_info;

pub struct PythonHgIdHistoryStore {
    py_store: PyObject,
}

impl PythonHgIdHistoryStore {
    pub fn new(py_store: PyObject) -> Self {
        PythonHgIdHistoryStore { py_store }
    }
}

impl HgIdHistoryStore for PythonHgIdHistoryStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let py_name = PyPathBuf::from(key.path.as_repo_path());
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

    fn refresh(&self) -> Result<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        match self
            .py_store
            .call_method(py, "markforrefresh", NoArgs, None)
        {
            Ok(_) => Ok(()),
            Err(py_err) => Err(PyErr::from(py_err).into()),
        }
    }
}

impl LocalStore for PythonHgIdHistoryStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let py_missing = PyList::new(py, &[]);
        for key in keys.iter() {
            match key {
                StoreKey::HgId(key) => {
                    let py_key = from_key_to_tuple(py, &key);
                    py_missing.insert(py, py_missing.len(py), py_key.into_object());
                }
                StoreKey::Content(_) => continue,
            }
        }

        let py_missing = self
            .py_store
            .call_method(py, "getmissing", (py_missing,), None)
            .map_err(|e| PyErr::from(e))?;
        let py_list = PyList::extract(py, &py_missing).map_err(|e| PyErr::from(e))?;
        let missing = py_list
            .iter(py)
            .map(|k| {
                Ok(StoreKey::from(
                    from_tuple_to_key(py, &k).map_err(|e| PyErr::from(e))?,
                ))
            })
            .collect::<Result<Vec<StoreKey>>>()?;
        Ok(missing)
    }
}
