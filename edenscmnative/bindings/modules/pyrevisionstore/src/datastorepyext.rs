// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cpython::{
    PyBytes, PyDict, PyErr, PyIterator, PyList, PyObject, PyResult, PyTuple, Python, PythonObject,
    ToPyObject,
};
use failure::Fallible;

use revisionstore::{DataStore, MutableDeltaStore, ToKeys};
use types::{Key, Node};

use crate::pythonutil::{
    from_base, from_delta_to_tuple, from_key, from_key_to_tuple, from_tuple_to_key, to_delta,
    to_key, to_metadata, to_pyerr,
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

impl<T: DataStore + ?Sized> DataStorePyExt for T {
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

impl<T: ToKeys + DataStore + ?Sized> IterableDataStorePyExt for T {
    fn iter_py(&self, py: Python) -> PyResult<Vec<PyTuple>> {
        let iter = self.to_keys().into_iter().map(|res| {
            let key = res?;
            let delta = self.get_delta(&key)?;
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
        iter.collect::<Fallible<Vec<PyTuple>>>()
            .map_err(|e| to_pyerr(py, &e.into()))
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

        self.add(&delta, &metadata).map_err(|e| to_pyerr(py, &e))?;
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
