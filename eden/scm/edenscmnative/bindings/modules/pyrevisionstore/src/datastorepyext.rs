/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use cpython::{
    PyBytes, PyDict, PyIterator, PyList, PyObject, PyResult, PyTuple, Python, PythonObject,
    ToPyObject,
};

use cpython_ext::ResultPyErrExt;
use revisionstore::{DataStore, MutableDeltaStore, RemoteDataStore, ToKeys};
use types::{Key, Node};

use crate::pythonutil::{
    from_base, from_delta_to_tuple, from_key, from_key_to_tuple, from_tuple_to_key, key_error,
    to_delta, to_key, to_metadata,
};

pub trait DataStorePyExt {
    fn get_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyBytes>;
    fn get_delta_chain_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyList>;
    fn get_delta_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyObject>;
    fn get_meta_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict>;
    fn get_missing_py(&self, py: Python, keys: &mut PyIterator) -> PyResult<PyList>;
}

pub trait IterableDataStorePyExt {
    fn iter_py(&self, py: Python) -> PyResult<Vec<PyTuple>>;
}

pub trait MutableDeltaStorePyExt: DataStorePyExt {
    fn add_py(
        &self,
        py: Python,
        name: &PyBytes,
        node: &PyBytes,
        deltabasenode: &PyBytes,
        delta: &PyBytes,
        metadata: Option<PyDict>,
    ) -> PyResult<PyObject>;
    fn flush_py(&self, py: Python) -> PyResult<PyObject>;
}

pub trait RemoteDataStorePyExt: RemoteDataStore {
    fn prefetch_py(&self, py: Python, keys: PyList) -> PyResult<PyObject>;
}

impl<T: DataStore + ?Sized> DataStorePyExt for T {
    fn get_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyBytes> {
        let key = to_key(py, name, node)?;
        let result = self
            .get(&key)
            .map_pyerr(py)?
            .ok_or_else(|| key_error(py, &key))?;

        Ok(PyBytes::new(py, &result[..]))
    }

    fn get_delta_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyObject> {
        let key = to_key(py, name, node)?;
        let delta = self
            .get_delta(&key)
            .map_pyerr(py)?
            .ok_or_else(|| key_error(py, &key))?;

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
        let deltachain = self
            .get_delta_chain(&key)
            .map_pyerr(py)?
            .ok_or_else(|| key_error(py, &key))?;

        let pychain = deltachain
            .iter()
            .map(|d| from_delta_to_tuple(py, &d))
            .collect::<Vec<PyObject>>();
        Ok(PyList::new(py, &pychain[..]))
    }

    fn get_meta_py(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict> {
        let key = to_key(py, name, node)?;
        let metadata = self
            .get_meta(&key)
            .map_pyerr(py)?
            .ok_or_else(|| key_error(py, &key))?;

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
            .collect::<PyResult<Vec<Key>>>()?;
        let missing = self.get_missing(&keys[..]).map_pyerr(py)?;

        let results = PyList::new(py, &[]);
        for key in missing {
            let key_tuple = from_key_to_tuple(py, &key);
            results.insert_item(py, results.len(py), key_tuple.into_object());
        }

        Ok(results)
    }
}

impl<T: ToKeys + DataStore + ?Sized> IterableDataStorePyExt for T {
    fn iter_py(&self, py: Python) -> PyResult<Vec<PyTuple>> {
        let iter = self.to_keys().into_iter().map(|res| {
            let key = res?;
            let delta = self.get_delta(&key)?.unwrap();
            let (name, node) = from_key(py, &key);
            let (_, base_node) = from_base(py, &delta);
            let tuple = (
                name.into_object(),
                node.into_object(),
                base_node.into_object(),
                delta.data.len().into_py_object(py),
            )
                .into_py_object(py);
            Ok(tuple)
        });
        iter.collect::<Result<Vec<PyTuple>>>().map_pyerr(py)
    }
}

impl<T: MutableDeltaStore + ?Sized> MutableDeltaStorePyExt for T {
    fn add_py(
        &self,
        py: Python,
        name: &PyBytes,
        node: &PyBytes,
        deltabasenode: &PyBytes,
        delta: &PyBytes,
        py_metadata: Option<PyDict>,
    ) -> PyResult<PyObject> {
        let delta = to_delta(py, name, node, deltabasenode, delta)?;

        let mut metadata = Default::default();
        if let Some(meta) = py_metadata {
            metadata = to_metadata(py, &meta)?;
        }

        self.add(&delta, &metadata).map_pyerr(py)?;
        Ok(Python::None(py))
    }

    fn flush_py(&self, py: Python) -> PyResult<PyObject> {
        let opt = self.flush().map_pyerr(py)?;
        let opt = opt
            .as_ref()
            .map(|path| encoding::path_to_local_bytes(path))
            .transpose()
            .map_pyerr(py)?;
        let opt = opt.map(|path| PyBytes::new(py, &path));
        Ok(opt.into_py_object(py))
    }
}

impl<T: RemoteDataStore + ?Sized> RemoteDataStorePyExt for T {
    fn prefetch_py(&self, py: Python, keys: PyList) -> PyResult<PyObject> {
        let keys = keys
            .iter(py)
            .map(|tuple| from_tuple_to_key(py, &tuple))
            .collect::<PyResult<Vec<Key>>>()?;
        self.prefetch(keys).map_pyerr(py)?;
        Ok(Python::None(py))
    }
}
