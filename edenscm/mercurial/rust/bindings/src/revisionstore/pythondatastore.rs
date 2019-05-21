// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::path::PathBuf;

use cpython::{
    FromPyObject, NoArgs, ObjectProtocol, PyBytes, PyDict, PyList, PyObject, PyResult, PyTuple,
    Python, PythonObject,
};
use failure::{format_err, Fallible};

use encoding::local_bytes_to_path;
use revisionstore::{DataStore, Delta, LocalStore, Metadata, MutableDeltaStore};
use types::Key;

use crate::revisionstore::pyerror::pyerr_to_error;
use crate::revisionstore::pythonutil::{
    bytes_from_tuple, from_delta_to_tuple, from_key_to_tuple, from_tuple_to_delta,
    from_tuple_to_key, to_key, to_pyerr,
};

pub struct PythonDataStore {
    py_store: PyObject,
}

pub struct PythonMutableDataPack {
    py_datapack: PyObject,
}

impl PythonDataStore {
    pub fn new(py_store: PyObject) -> Self {
        PythonDataStore { py_store }
    }
}

// All accesses are protected by the GIL, so it's thread safe. This is required because it is
// eventually stored on the `datastore` python class and Rust CPython requires that stored members
// implement Send.
unsafe impl Send for PythonDataStore {}
unsafe impl Send for PythonMutableDataPack {}

impl DataStore for PythonDataStore {
    fn get(&self, key: &Key) -> Fallible<Vec<u8>> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let py_name = PyBytes::new(py, key.path.as_byte_slice());
        let py_node = PyBytes::new(py, key.node.as_ref());

        let py_data = self
            .py_store
            .call_method(py, "get", (py_name, py_node), None)
            .map_err(|e| pyerr_to_error(py, e))?;

        let py_bytes = PyBytes::extract(py, &py_data).map_err(|e| pyerr_to_error(py, e))?;

        Ok(py_bytes.data(py).to_vec())
    }

    fn get_delta(&self, key: &Key) -> Fallible<Delta> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let py_name = PyBytes::new(py, key.path.as_byte_slice());
        let py_node = PyBytes::new(py, key.node.as_ref());
        let py_delta = self
            .py_store
            .call_method(py, "getdelta", (py_name, py_node), None)
            .map_err(|e| pyerr_to_error(py, e))?;
        let py_tuple = PyTuple::extract(py, &py_delta).map_err(|e| pyerr_to_error(py, e))?;

        let py_name = bytes_from_tuple(py, &py_tuple, 0)?;
        let py_node = bytes_from_tuple(py, &py_tuple, 1)?;
        let py_delta_name = bytes_from_tuple(py, &py_tuple, 2)?;
        let py_delta_node = bytes_from_tuple(py, &py_tuple, 3)?;
        let py_bytes = bytes_from_tuple(py, &py_tuple, 4)?;

        let base_key =
            to_key(py, &py_delta_name, &py_delta_node).map_err(|e| pyerr_to_error(py, e))?;
        Ok(Delta {
            data: py_bytes.data(py).to_vec().into(),
            base: if base_key.node.is_null() {
                None
            } else {
                Some(base_key)
            },
            key: to_key(py, &py_name, &py_node).map_err(|e| pyerr_to_error(py, e))?,
        })
    }

    fn get_delta_chain(&self, key: &Key) -> Fallible<Vec<Delta>> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let py_name = PyBytes::new(py, key.path.as_byte_slice());
        let py_node = PyBytes::new(py, key.node.as_ref());
        let py_chain = self
            .py_store
            .call_method(py, "getdeltachain", (py_name, py_node), None)
            .map_err(|e| pyerr_to_error(py, e))?;
        let py_list = PyList::extract(py, &py_chain).map_err(|e| pyerr_to_error(py, e))?;
        let deltas = py_list
            .iter(py)
            .map(|b| from_tuple_to_delta(py, &b).map_err(|e| pyerr_to_error(py, e).into()))
            .collect::<Fallible<Vec<Delta>>>()?;
        Ok(deltas)
    }

    fn get_meta(&self, key: &Key) -> Fallible<Metadata> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let py_name = PyBytes::new(py, key.path.as_byte_slice());
        let py_node = PyBytes::new(py, key.node.as_ref());
        let py_meta = self
            .py_store
            .call_method(py, "getmeta", (py_name, py_node), None)
            .map_err(|e| pyerr_to_error(py, e))?;
        let py_dict = PyDict::extract(py, &py_meta).map_err(|e| pyerr_to_error(py, e))?;

        Ok(Metadata {
            flags: match py_dict.get_item(py, "f") {
                Some(x) => Some(u64::extract(py, &x).map_err(|e| pyerr_to_error(py, e))?),
                None => None,
            },
            size: match py_dict.get_item(py, "s") {
                Some(x) => Some(u64::extract(py, &x).map_err(|e| pyerr_to_error(py, e))?),
                None => None,
            },
        })
    }
}

impl LocalStore for PythonDataStore {
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
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
            .map_err(|e| pyerr_to_error(py, e))?;
        let py_list = PyList::extract(py, &py_missing).map_err(|e| pyerr_to_error(py, e))?;
        let missing = py_list
            .iter(py)
            .map(|k| from_tuple_to_key(py, &k).map_err(|e| pyerr_to_error(py, e).into()))
            .collect::<Fallible<Vec<Key>>>()?;
        Ok(missing)
    }
}

impl PythonMutableDataPack {
    pub fn new(py_datapack: PyObject) -> PyResult<Self> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let py_type = py_datapack.get_type(py);
        let name = py_type.name(py);
        if name != "mutabledatapack" {
            Err(to_pyerr(
                py,
                &format_err!(
                    "A 'mutabledatapack' object was expected but got a '{}' object",
                    name
                ),
            ))
        } else {
            Ok(PythonMutableDataPack { py_datapack })
        }
    }

    fn build_add_tuple(&self, py: Python, delta: &Delta, metadata: &Metadata) -> PyResult<PyTuple> {
        let py_delta = from_delta_to_tuple(py, delta);
        let py_delta = PyTuple::extract(py, &py_delta)?;
        let py_name = PyBytes::extract(py, &py_delta.get_item(py, 0))?;
        let py_node = PyBytes::extract(py, &py_delta.get_item(py, 1))?;
        let py_delta_node = PyBytes::extract(py, &py_delta.get_item(py, 3))?;
        let py_bytes = PyBytes::extract(py, &py_delta.get_item(py, 4))?;
        let py_meta = PyDict::new(py);
        if let Some(size) = metadata.size {
            py_meta.set_item(py, "s", size)?;
        }
        if let Some(flags) = metadata.flags {
            py_meta.set_item(py, "f", flags)?;
        }

        Ok(PyTuple::new(
            py,
            &vec![
                py_name.into_object(),
                py_node.into_object(),
                py_delta_node.into_object(),
                py_bytes.into_object(),
                py_meta.into_object(),
            ],
        ))
    }
}

impl MutableDeltaStore for PythonMutableDataPack {
    fn add(&mut self, delta: &Delta, metadata: &Metadata) -> Fallible<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let py_tuple = self
            .build_add_tuple(py, delta, metadata)
            .map_err(|e| pyerr_to_error(py, e))?;

        self.py_datapack
            .call_method(py, "add", py_tuple, None)
            .map_err(|e| pyerr_to_error(py, e))?;
        Ok(())
    }

    fn flush(&mut self) -> Fallible<Option<PathBuf>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let py_path = self
            .py_datapack
            .call_method(py, "flush", NoArgs, None)
            .map_err(|e| pyerr_to_error(py, e))?;
        let py_path = PyBytes::extract(py, &py_path).map_err(|e| pyerr_to_error(py, e))?;
        Ok(Some(local_bytes_to_path(py_path.data(py))?.into_owned()))
    }
}
