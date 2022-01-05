/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Result;
use cpython::PyBytes;
use cpython::PyDict;
use cpython::PyIterator;
use cpython::PyList;
use cpython::PyObject;
use cpython::PyResult;
use cpython::PyTuple;
use cpython::Python;
use cpython::PythonObject;
use cpython::ToPyObject;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use revisionstore::datastore::Delta;
use revisionstore::datastore::StoreResult;
use revisionstore::ContentDataStore;
use revisionstore::ContentHash;
use revisionstore::HgIdDataStore;
use revisionstore::HgIdMutableDeltaStore;
use revisionstore::RemoteDataStore;
use revisionstore::StoreKey;
use revisionstore::ToKeys;
use types::Node;

use crate::pythonutil::from_base;
use crate::pythonutil::from_delta_to_tuple;
use crate::pythonutil::from_key;
use crate::pythonutil::from_key_to_tuple;
use crate::pythonutil::from_tuple_to_key;
use crate::pythonutil::key_error;
use crate::pythonutil::to_delta;
use crate::pythonutil::to_key;
use crate::pythonutil::to_metadata;

pub trait HgIdDataStorePyExt {
    fn get_py(&self, py: Python, name: &PyPath, node: &PyBytes) -> PyResult<PyBytes>;
    fn get_delta_chain_py(&self, py: Python, name: &PyPath, node: &PyBytes) -> PyResult<PyList>;
    fn get_delta_py(&self, py: Python, name: &PyPath, node: &PyBytes) -> PyResult<PyObject>;
    fn get_meta_py(&self, py: Python, name: &PyPath, node: &PyBytes) -> PyResult<PyDict>;
    fn get_missing_py(&self, py: Python, keys: &mut PyIterator) -> PyResult<PyList>;
    fn refresh_py(&self, py: Python) -> PyResult<PyNone>;
}

pub trait ContentDataStorePyExt {
    fn blob_py(&self, py: Python, name: &PyPath, node: &PyBytes) -> PyResult<PyBytes>;
    fn metadata_py(&self, py: Python, name: &PyPath, node: &PyBytes) -> PyResult<PyDict>;
}

pub trait IterableHgIdDataStorePyExt {
    fn iter_py(&self, py: Python) -> PyResult<Vec<PyTuple>>;
}

pub trait HgIdMutableDeltaStorePyExt: HgIdDataStorePyExt {
    fn add_py(
        &self,
        py: Python,
        name: &PyPath,
        node: &PyBytes,
        deltabasenode: &PyBytes,
        delta: &PyBytes,
        metadata: Option<PyDict>,
    ) -> PyResult<PyObject>;
    fn flush_py(&self, py: Python) -> PyResult<Option<Vec<PyPathBuf>>>;
}

pub trait RemoteDataStorePyExt: RemoteDataStore {
    fn prefetch_py(&self, py: Python, keys: PyList) -> PyResult<PyObject>;
    fn upload_py(&self, py: Python, keys: PyList) -> PyResult<PyList>;
}

impl<T: HgIdDataStore + ?Sized> HgIdDataStorePyExt for T {
    fn get_py(&self, py: Python, name: &PyPath, node: &PyBytes) -> PyResult<PyBytes> {
        let key = StoreKey::hgid(to_key(py, name, node)?);
        let result = py.allow_threads(|| self.get(key)).map_pyerr(py)?;
        match result {
            StoreResult::Found(data) => Ok(PyBytes::new(py, &data[..])),
            StoreResult::NotFound(key) => Err(key_error(py, &key)),
        }
    }

    fn get_delta_py(&self, py: Python, name: &PyPath, node: &PyBytes) -> PyResult<PyObject> {
        let key = to_key(py, name, node)?;
        let storekey = StoreKey::hgid(key.clone());

        let res = py.allow_threads(|| self.get(storekey)).map_pyerr(py)?;
        let data = match res {
            StoreResult::Found(data) => data,
            StoreResult::NotFound(key) => return Err(key_error(py, &key)),
        };

        let delta = Delta {
            data: data.into(),
            base: None,
            key: key.clone(),
        };

        let base_name = PyPathBuf::from(key.path.as_repo_path());
        let base_node = PyBytes::new(py, Node::null_id().as_ref());

        let bytes = PyBytes::new(py, &delta.data);
        let meta = self.get_meta_py(py.clone(), &name, &node)?;
        Ok((
            bytes.into_object(),
            base_name.to_py_object(py).into_object(),
            base_node.into_object(),
            meta.into_object(),
        )
            .into_py_object(py)
            .into_object())
    }

    fn get_delta_chain_py(&self, py: Python, name: &PyPath, node: &PyBytes) -> PyResult<PyList> {
        let key = to_key(py, name, node)?;
        let storekey = StoreKey::hgid(key.clone());

        let res = py.allow_threads(|| self.get(storekey)).map_pyerr(py)?;

        let data = match res {
            StoreResult::Found(data) => data,
            StoreResult::NotFound(key) => return Err(key_error(py, &key)),
        };

        let delta = Delta {
            data: data.into(),
            base: None,
            key,
        };

        let deltachain = vec![delta];

        let pychain = deltachain
            .iter()
            .map(|d| from_delta_to_tuple(py, &d))
            .collect::<Vec<PyObject>>();
        Ok(PyList::new(py, &pychain[..]))
    }

    fn get_meta_py(&self, py: Python, name: &PyPath, node: &PyBytes) -> PyResult<PyDict> {
        let key = StoreKey::hgid(to_key(py, name, node)?);
        let res = py.allow_threads(|| self.get_meta(key)).map_pyerr(py)?;

        let metadata = match res {
            StoreResult::Found(metadata) => metadata,
            StoreResult::NotFound(key) => return Err(key_error(py, &key)),
        };

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
                Ok(k) => Ok(StoreKey::from(from_tuple_to_key(py, &k)?)),
                Err(e) => Err(e),
            })
            .collect::<PyResult<Vec<StoreKey>>>()?;
        let missing = py.allow_threads(|| self.get_missing(&keys)).map_pyerr(py)?;

        let results = PyList::new(py, &[]);
        for key in missing {
            match key {
                StoreKey::HgId(key) => {
                    let key_tuple = from_key_to_tuple(py, &key);
                    results.append(py, key_tuple.into_object());
                }
                StoreKey::Content(_, key) => {
                    // This is tricky, when ran on the ContentStore, Python will only call this
                    // method when the network is flaky and the connection to the server is
                    // severed. In this case, the Python code will attempt to not continue from
                    // where it left of, and thus calls datastore.getmissing. It is very possible
                    // that some LFS pointers were received but we didn't have a chance to receive
                    // the blobs themself, leading the ContentStore code to logically indicate that
                    // the blob is missing, but not the pointer.
                    //
                    // It might be worth moving the retry logic from Python to be driven in Rust
                    // entirely as this would eliminate this hack entirely.
                    if let Some(key) = key {
                        let key_tuple = from_key_to_tuple(py, &key);
                        results.append(py, key_tuple.into_object());
                    }
                }
            }
        }

        Ok(results)
    }

    fn refresh_py(&self, py: Python) -> PyResult<PyNone> {
        self.refresh().map_pyerr(py)?;
        Ok(PyNone)
    }
}

impl<T: ContentDataStore + ?Sized> ContentDataStorePyExt for T {
    fn blob_py(&self, py: Python, name: &PyPath, node: &PyBytes) -> PyResult<PyBytes> {
        let key = StoreKey::hgid(to_key(py, name, node)?);
        let res = py.allow_threads(|| self.blob(key)).map_pyerr(py)?;
        match res {
            StoreResult::Found(blob) => Ok(PyBytes::new(py, blob.as_ref())),
            StoreResult::NotFound(key) => Err(key_error(py, &key)),
        }
    }

    fn metadata_py(&self, py: Python, name: &PyPath, node: &PyBytes) -> PyResult<PyDict> {
        let key = StoreKey::hgid(to_key(py, name, node)?);
        let res = py.allow_threads(|| self.metadata(key)).map_pyerr(py)?;

        let meta = match res {
            StoreResult::Found(meta) => meta,
            StoreResult::NotFound(key) => return Err(key_error(py, &key)),
        };

        let metadict = PyDict::new(py);
        metadict.set_item(py, "size", meta.size)?;
        match meta.hash {
            ContentHash::Sha256(hash) => {
                metadict.set_item(py, "sha256", PyBytes::new(py, hash.as_ref()))?
            }
        }
        metadict.set_item(py, "isbinary", meta.is_binary)?;

        Ok(metadict)
    }
}

impl<T: ToKeys + HgIdDataStore + ?Sized> IterableHgIdDataStorePyExt for T {
    fn iter_py(&self, py: Python) -> PyResult<Vec<PyTuple>> {
        let iter = py.allow_threads(|| self.to_keys()).into_iter().map(|res| {
            let key = res?;
            let res = py.allow_threads(|| self.get(StoreKey::hgid(key.clone())))?;
            let data = match res {
                StoreResult::Found(data) => data,
                StoreResult::NotFound(_) => return Err(format_err!("Key {:?} not found", key)),
            };
            let delta = Delta {
                data: data.into(),
                base: None,
                key: key.clone(),
            };
            let (name, node) = from_key(py, &key);
            let (_, base_node) = from_base(py, &delta);
            let tuple = (
                name.to_py_object(py).into_object(),
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

impl<T: HgIdMutableDeltaStore + ?Sized> HgIdMutableDeltaStorePyExt for T {
    fn add_py(
        &self,
        py: Python,
        name: &PyPath,
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

        py.allow_threads(|| self.add(&delta, &metadata))
            .map_pyerr(py)?;
        Ok(Python::None(py))
    }

    fn flush_py(&self, py: Python) -> PyResult<Option<Vec<PyPathBuf>>> {
        let opt = py.allow_threads(|| self.flush()).map_pyerr(py)?;
        let opt = opt
            .map(|path| path.into_iter().map(|p| p.try_into()).collect())
            .transpose()
            .map_pyerr(py)?;
        Ok(opt)
    }
}

impl<T: RemoteDataStore + ?Sized> RemoteDataStorePyExt for T {
    fn prefetch_py(&self, py: Python, keys: PyList) -> PyResult<PyObject> {
        let keys = keys
            .iter(py)
            .map(|tuple| Ok(StoreKey::from(from_tuple_to_key(py, &tuple)?)))
            .collect::<PyResult<Vec<StoreKey>>>()?;
        py.allow_threads(|| self.prefetch(&keys)).map_pyerr(py)?;
        Ok(Python::None(py))
    }

    fn upload_py(&self, py: Python, keys: PyList) -> PyResult<PyList> {
        let keys = keys
            .iter(py)
            .map(|tuple| Ok(StoreKey::from(from_tuple_to_key(py, &tuple)?)))
            .collect::<PyResult<Vec<StoreKey>>>()?;
        let not_uploaded = py.allow_threads(|| self.upload(&keys)).map_pyerr(py)?;

        let results = PyList::new(py, &[]);
        for key in not_uploaded {
            match key {
                StoreKey::HgId(key) => {
                    let key_tuple = from_key_to_tuple(py, &key);
                    results.append(py, key_tuple.into_object());
                }
                StoreKey::Content(_, _) => {
                    return Err(format_err!("Unsupported key: {:?}", key)).map_pyerr(py);
                }
            }
        }

        Ok(results)
    }
}
