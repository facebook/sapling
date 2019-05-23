// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cpython::{
    PyBytes, PyDict, PyErr, PyIterator, PyList, PyObject, PyResult, Python, PythonObject,
    ToPyObject,
};

use revisionstore::datastore::DataStore;
use types::{Key, Node};

use crate::revisionstore::pythonutil::{
    from_delta_to_tuple, from_key, from_key_to_tuple, from_tuple_to_key, to_key, to_pyerr,
};

pub trait DataStorePyExt {
    fn get_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyBytes>;
    fn get_delta_chain_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyList>;
    fn get_delta_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyObject>;
    fn get_meta_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict>;
    fn get_missing_py(&self, py: Python, keys: &mut PyIterator) -> PyResult<PyList>;
}

impl<T: DataStore> DataStorePyExt for T {
    fn get_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyBytes> {
        let key = to_key(py, name, node)?;
        let result = self.get(&key).map_err(|e| to_pyerr(py, &e))?;

        Ok(PyBytes::new(py, &result[..]))
    }

    fn get_delta_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyObject> {
        let key = to_key(py, name, node)?;
        let delta = self.get_delta(&key).map_err(|e| to_pyerr(py, &e))?;

        let (base_name, base_node) = if let Some(key) = delta.base {
            from_key(py, &key)
        } else {
            (
                PyBytes::new(py, key.path.as_byte_slice()),
                PyBytes::new(py, Node::null_id().as_ref()),
            )
        };

        let bytes = PyBytes::new(py, &delta.data);
        let meta = self.get_meta_py(py.clone(), &name, &node)?;
        Ok((
            bytes.into_object(),
            base_name.into_object(),
            base_node.into_object(),
            meta.into_object(),
        )
            .into_py_object(py)
            .into_object())
    }

    fn get_delta_chain_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyList> {
        let key = to_key(py, name, node)?;
        let deltachain = self.get_delta_chain(&key).map_err(|e| to_pyerr(py, &e))?;
        let pychain = deltachain
            .iter()
            .map(|d| from_delta_to_tuple(py, &d))
            .collect::<Vec<PyObject>>();
        Ok(PyList::new(py, &pychain[..]))
    }

    fn get_meta_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict> {
        let key = to_key(py, name, node)?;
        let metadata = self.get_meta(&key).map_err(|e| to_pyerr(py, &e))?;
        let metadict = PyDict::new(py);
        if let Some(size) = metadata.size {
            metadict.set_item(py, "s", size)?;
        }
        if let Some(flags) = metadata.flags {
            metadict.set_item(py, "f", flags)?;
        }

        Ok(metadict)
    }

    fn get_missing_py(&self, py: Python, keys: &mut PyIterator) -> PyResult<PyList> {
        // Copy the PyObjects into a vector so we can get a reference iterator.
        // This lets us get a Vector of Keys without copying the strings.
        let keys = keys
            .map(|k| match k {
                Ok(k) => from_tuple_to_key(py, &k),
                Err(e) => Err(e),
            })
            .collect::<Result<Vec<Key>, PyErr>>()?;
        let missing = self.get_missing(&keys[..]).map_err(|e| to_pyerr(py, &e))?;

        let results = PyList::new(py, &[]);
        for key in missing {
            let key_tuple = from_key_to_tuple(py, &key);
            results.insert_item(py, results.len(py), key_tuple.into_object());
        }

        Ok(results)
    }
}
