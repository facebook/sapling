/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use cpython::{
    exc, FromPyObject, ObjectProtocol, PyBytes, PyDict, PyList, PyObject, PyTuple, Python,
    PythonObject, PythonObjectWithTypeObject,
};

use cpython_ext::{PyErr, PyPathBuf};
use revisionstore::{Delta, HgIdDataStore, LocalStore, Metadata, RemoteDataStore, StoreKey};
use types::Key;

use crate::pythonutil::{
    bytes_from_tuple, from_key_to_tuple, from_tuple_to_delta, from_tuple_to_key, path_from_tuple,
    to_key, to_metadata,
};

pub struct PythonHgIdDataStore {
    py_store: PyObject,
}

impl PythonHgIdDataStore {
    pub fn new(py_store: PyObject) -> Self {
        PythonHgIdDataStore { py_store }
    }
}

impl HgIdDataStore for PythonHgIdDataStore {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let py_name = PyPathBuf::from(key.path.as_repo_path());
        let py_node = PyBytes::new(py, key.hgid.as_ref());

        let py_data = match self
            .py_store
            .call_method(py, "get", (py_name, py_node), None)
        {
            Ok(data) => data,
            Err(py_err) => {
                if py_err.get_type(py) == exc::KeyError::type_object(py) {
                    return Ok(None);
                } else {
                    return Err(PyErr::from(py_err).into());
                }
            }
        };

        let py_bytes = PyBytes::extract(py, &py_data).map_err(|e| PyErr::from(e))?;

        Ok(Some(py_bytes.data(py).to_vec()))
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let py_name = PyPathBuf::from(key.path.as_repo_path());
        let py_node = PyBytes::new(py, key.hgid.as_ref());
        let py_delta = match self
            .py_store
            .call_method(py, "getdelta", (py_name, py_node), None)
        {
            Ok(data) => data,
            Err(py_err) => {
                if py_err.get_type(py) == exc::KeyError::type_object(py) {
                    return Ok(None);
                } else {
                    return Err(PyErr::from(py_err).into());
                }
            }
        };
        let py_tuple = PyTuple::extract(py, &py_delta).map_err(|e| PyErr::from(e))?;

        let py_name = path_from_tuple(py, &py_tuple, 0)?;
        let py_node = bytes_from_tuple(py, &py_tuple, 1)?;
        let py_delta_name = path_from_tuple(py, &py_tuple, 2)?;
        let py_delta_node = bytes_from_tuple(py, &py_tuple, 3)?;
        let py_bytes = bytes_from_tuple(py, &py_tuple, 4)?;

        let base_key = to_key(py, &py_delta_name, &py_delta_node).map_err(|e| PyErr::from(e))?;
        Ok(Some(Delta {
            data: py_bytes.data(py).to_vec().into(),
            base: if base_key.hgid.is_null() {
                None
            } else {
                Some(base_key)
            },
            key: to_key(py, &py_name, &py_node).map_err(|e| PyErr::from(e))?,
        }))
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let py_name = PyPathBuf::from(key.path.as_repo_path());
        let py_node = PyBytes::new(py, key.hgid.as_ref());
        let py_chain =
            match self
                .py_store
                .call_method(py, "getdeltachain", (py_name, py_node), None)
            {
                Ok(data) => data,
                Err(py_err) => {
                    if py_err.get_type(py) == exc::KeyError::type_object(py) {
                        return Ok(None);
                    } else {
                        return Err(PyErr::from(py_err).into());
                    }
                }
            };
        let py_list = PyList::extract(py, &py_chain).map_err(|e| PyErr::from(e))?;
        let deltas = py_list
            .iter(py)
            .map(|b| from_tuple_to_delta(py, &b).map_err(|e| PyErr::from(e).into()))
            .collect::<Result<Vec<Delta>>>()?;
        Ok(Some(deltas))
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let py_name = PyPathBuf::from(key.path.as_repo_path());
        let py_node = PyBytes::new(py, key.hgid.as_ref());
        let py_meta = match self
            .py_store
            .call_method(py, "getmeta", (py_name, py_node), None)
        {
            Ok(data) => data,
            Err(py_err) => {
                if py_err.get_type(py) == exc::KeyError::type_object(py) {
                    return Ok(None);
                } else {
                    return Err(PyErr::from(py_err).into());
                }
            }
        };
        let py_dict = PyDict::extract(py, &py_meta).map_err(|e| PyErr::from(e))?;
        to_metadata(py, &py_dict)
            .map_err(|e| PyErr::from(e).into())
            .map(Some)
    }
}

impl RemoteDataStore for PythonHgIdDataStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let keys = keys
            .into_iter()
            .filter_map(|key| match key {
                StoreKey::HgId(key) => {
                    let py_name = PyPathBuf::from(key.path.as_repo_path());
                    let py_node = PyBytes::new(py, key.hgid.as_ref());
                    Some((py_name, py_node))
                }
                StoreKey::Content(_, _) => None,
            })
            .collect::<Vec<_>>();

        self.py_store
            .call_method(py, "prefetch", (keys,), None)
            .map_err(|e| PyErr::from(e))?;

        Ok(())
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }
}

impl LocalStore for PythonHgIdDataStore {
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
                StoreKey::Content(_, _) => continue,
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
