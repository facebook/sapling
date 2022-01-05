/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

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
use revisionstore::Delta;
use revisionstore::HgIdDataStore;
use revisionstore::HgIdMutableDeltaStore;
use revisionstore::LegacyStore;
use revisionstore::LocalStore;
use revisionstore::Metadata;
use revisionstore::RemoteDataStore;
use revisionstore::RepackLocation;
use revisionstore::StoreKey;
use revisionstore::StoreResult;
use types::Key;
use types::RepoPathBuf;

use crate::pythonutil::from_key_to_tuple;
use crate::pythonutil::from_tuple_to_key;
use crate::pythonutil::to_metadata;

pub struct PythonHgIdDataStore {
    py_store: PyObject,
}

impl PythonHgIdDataStore {
    pub fn new(py_store: PyObject) -> Self {
        PythonHgIdDataStore { py_store }
    }
}

impl HgIdDataStore for PythonHgIdDataStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        let key = match key {
            StoreKey::HgId(key) => key,
            contentkey => return Ok(StoreResult::NotFound(contentkey)),
        };

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
                    return Ok(StoreResult::NotFound(StoreKey::hgid(key)));
                } else {
                    return Err(PyErr::from(py_err).into());
                }
            }
        };

        let py_bytes = PyBytes::extract(py, &py_data).map_err(|e| PyErr::from(e))?;

        Ok(StoreResult::Found(py_bytes.data(py).to_vec()))
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        let key = match key {
            StoreKey::HgId(key) => key,
            contentkey => return Ok(StoreResult::NotFound(contentkey)),
        };

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
                    return Ok(StoreResult::NotFound(StoreKey::hgid(key)));
                } else {
                    return Err(PyErr::from(py_err).into());
                }
            }
        };
        let py_dict = PyDict::extract(py, &py_meta).map_err(|e| PyErr::from(e))?;
        to_metadata(py, &py_dict)
            .map_err(|e| PyErr::from(e).into())
            .map(StoreResult::Found)
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

impl RemoteDataStore for PythonHgIdDataStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let py_keys = keys
            .iter()
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
            .call_method(py, "prefetch", (py_keys,), None)
            .map_err(|e| PyErr::from(e))?;

        self.get_missing(&keys)
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

// Dummy implementation just to satisfy ManifestStore which consumes the sub-traits of LegacyStore
// which PythonHgIdDataStore already implements. This is only required for tests.
impl LegacyStore for PythonHgIdDataStore {
    fn get_file_content(&self, _key: &Key) -> Result<Option<minibytes::Bytes>> {
        unimplemented!("")
    }

    fn get_logged_fetches(&self) -> HashSet<RepoPathBuf> {
        unimplemented!("")
    }

    fn get_shared_mutable(&self) -> Arc<dyn HgIdMutableDeltaStore> {
        unimplemented!("")
    }

    fn add_pending(
        &self,
        _key: &Key,
        _data: minibytes::Bytes,
        _meta: Metadata,
        _location: RepackLocation,
    ) -> Result<()> {
        unimplemented!("")
    }

    fn commit_pending(&self, _location: RepackLocation) -> Result<Option<Vec<PathBuf>>> {
        unimplemented!("")
    }
}

// Dummy implementation just to satisfy ManifestStore which consumes the sub-traits of LegacyStore
// which PythonHgIdDataStore already implements. This is only required for tests.
impl HgIdMutableDeltaStore for PythonHgIdDataStore {
    fn add(&self, _delta: &Delta, _metadata: &Metadata) -> Result<()> {
        unimplemented!()
    }
    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        unimplemented!()
    }
}
