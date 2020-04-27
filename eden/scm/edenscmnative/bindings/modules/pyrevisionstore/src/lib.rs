/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! revisionstore - Python interop layer for a Mercurial data and history store

#![allow(non_camel_case_types)]

use std::{
    convert::TryInto,
    fs::read_dir,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{format_err, Error};
use cpython::*;
use parking_lot::RwLock;

use cpython_ext::{PyErr, PyNone, PyPath, PyPathBuf, ResultPyErrExt, Str};
use pyconfigparser::config;
use revisionstore::{
    repack, ContentStore, ContentStoreBuilder, CorruptionPolicy, DataPack, DataPackStore,
    DataPackVersion, Delta, HgIdDataStore, HgIdHistoryStore, HgIdMutableDeltaStore,
    HgIdMutableHistoryStore, HgIdRemoteStore, HistoryPack, HistoryPackStore, HistoryPackVersion,
    IndexedLogHgIdDataStore, IndexedLogHgIdHistoryStore, IndexedlogRepair, LocalStore,
    MemcacheStore, Metadata, MetadataStore, MetadataStoreBuilder, MutableDataPack,
    MutableHistoryPack, RemoteDataStore, RemoteHistoryStore, RepackKind, RepackLocation, StoreKey,
};
use types::{Key, NodeInfo};

use crate::{
    datastorepyext::{
        ContentDataStorePyExt, HgIdDataStorePyExt, HgIdMutableDeltaStorePyExt,
        IterableHgIdDataStorePyExt, RemoteDataStorePyExt,
    },
    historystorepyext::{
        HgIdHistoryStorePyExt, HgIdMutableHistoryStorePyExt, IterableHgIdHistoryStorePyExt,
        RemoteHistoryStorePyExt,
    },
    pythonutil::from_key,
};

mod datastorepyext;
mod historystorepyext;
mod pythondatastore;
mod pythonutil;

type Result<T, E = Error> = std::result::Result<T, E>;

pub use crate::pythondatastore::PythonHgIdDataStore;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "revisionstore"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<datapack>(py)?;
    m.add_class::<datapackstore>(py)?;
    m.add_class::<historypack>(py)?;
    m.add_class::<historypackstore>(py)?;
    m.add_class::<indexedlogdatastore>(py)?;
    m.add_class::<indexedloghistorystore>(py)?;
    m.add_class::<mutabledeltastore>(py)?;
    m.add_class::<mutablehistorystore>(py)?;
    m.add_class::<pyremotestore>(py)?;
    m.add_class::<contentstore>(py)?;
    m.add_class::<metadatastore>(py)?;
    m.add_class::<memcachestore>(py)?;
    m.add(
        py,
        "repack",
        py_fn!(
            py,
            repack_py(
                packpath: &PyPath,
                stores: Option<(contentstore, metadatastore)>,
                full: bool,
                shared: bool
            )
        ),
    )?;
    Ok(m)
}

fn repack_py(
    py: Python,
    packpath: &PyPath,
    stores: Option<(contentstore, metadatastore)>,
    full: bool,
    shared: bool,
) -> PyResult<PyNone> {
    let stores = stores.map(|(content, metadata)| (content.to_inner(py), metadata.to_inner(py)));

    let kind = if full {
        RepackKind::Full
    } else {
        RepackKind::Incremental
    };

    let location = if shared {
        RepackLocation::Shared
    } else {
        RepackLocation::Local
    };

    repack(packpath.to_path_buf(), stores, kind, location).map_pyerr(py)?;

    Ok(PyNone)
}

py_class!(class datapack |py| {
    data store: Box<DataPack>;

    def __new__(
        _cls,
        path: &PyPath
    ) -> PyResult<datapack> {
        datapack::create_instance(
            py,
            Box::new(DataPack::new(path).map_pyerr(py)?),
        )
    }

    def path(&self) -> PyResult<PyPathBuf> {
        self.store(py).base_path().try_into().map_pyerr(py)
    }

    def packpath(&self) -> PyResult<PyPathBuf> {
        self.store(py).pack_path().try_into().map_pyerr(py)
    }

    def indexpath(&self) -> PyResult<PyPathBuf> {
        self.store(py).index_path().try_into().map_pyerr(py)
    }

    def get(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyBytes> {
        let store = self.store(py);
        store.get_py(py, &name, node)
    }

    def getdelta(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyObject> {
        let store = self.store(py);
        store.get_delta_py(py, &name, node)
    }

    def getdeltachain(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyList> {
        let store = self.store(py);
        store.get_delta_chain_py(py, &name, node)
    }

    def getmeta(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyDict> {
        let store = self.store(py);
        store.get_meta_py(py, &name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        let store = self.store(py);
        store.get_missing_py(py, &mut keys.iter(py)?)
    }

    def iterentries(&self) -> PyResult<Vec<PyTuple>> {
        let store = self.store(py);
        store.iter_py(py)
    }
});

/// Scan the filesystem for files with `extensions`, and compute their size.
fn compute_store_size<P: AsRef<Path>>(
    storepath: P,
    extensions: Vec<&str>,
) -> Result<(usize, usize)> {
    let dirents = read_dir(storepath)?;

    assert_eq!(extensions.len(), 2);

    let mut count = 0;
    let mut size = 0;

    for dirent in dirents {
        let dirent = dirent?;
        let path = dirent.path();

        if let Some(file_ext) = path.extension() {
            for extension in &extensions {
                if extension == &file_ext {
                    size += dirent.metadata()?.len();
                    count += 1;
                    break;
                }
            }
        }
    }

    // We did count the indexes too, but we do not want them counted.
    count /= 2;

    Ok((size as usize, count))
}

py_class!(class datapackstore |py| {
    data store: Box<DataPackStore>;
    data path: PathBuf;

    def __new__(_cls, path: &PyPath, deletecorruptpacks: bool = false) -> PyResult<datapackstore> {
        let corruption_policy = if deletecorruptpacks {
            CorruptionPolicy::REMOVE
        } else {
            CorruptionPolicy::IGNORE
        };

        datapackstore::create_instance(py, Box::new(DataPackStore::new(path, corruption_policy)), path.to_path_buf())
    }

    def get(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyBytes> {
        self.store(py).get_py(py, &name, node)
    }

    def getmeta(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyDict> {
        self.store(py).get_meta_py(py, &name, node)
    }

    def getdelta(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyObject> {
        self.store(py).get_delta_py(py, &name, node)
    }

    def getdeltachain(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyList> {
        self.store(py).get_delta_chain_py(py, &name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        self.store(py).get_missing_py(py, &mut keys.iter(py)?)
    }

    def markforrefresh(&self) -> PyResult<PyObject> {
        self.store(py).force_rescan();
        Ok(Python::None(py))
    }

    def getmetrics(&self) -> PyResult<PyDict> {
        let (size, count) = match compute_store_size(self.path(py), vec!["datapack", "dataidx"]) {
            Ok((size, count)) => (size, count),
            Err(_) => (0, 0),
        };

        let res = PyDict::new(py);
        res.set_item(py, "numpacks", count)?;
        res.set_item(py, "totalpacksize", size)?;
        Ok(res)
    }
});

py_class!(class historypack |py| {
    data store: Box<HistoryPack>;

    def __new__(
        _cls,
        path: &PyPath
    ) -> PyResult<historypack> {
        historypack::create_instance(
            py,
            Box::new(HistoryPack::new(path.as_path()).map_pyerr(py)?),
        )
    }

    def path(&self) -> PyResult<PyPathBuf> {
        self.store(py).base_path().try_into().map_pyerr(py)
    }

    def packpath(&self) -> PyResult<PyPathBuf> {
        self.store(py).pack_path().try_into().map_pyerr(py)
    }

    def indexpath(&self) -> PyResult<PyPathBuf> {
        self.store(py).index_path().try_into().map_pyerr(py)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        let store = self.store(py);
        store.get_missing_py(py, &mut keys.iter(py)?)
    }

    def getnodeinfo(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyTuple> {
        let store = self.store(py);
        store.get_node_info_py(py, &name, node)
    }

    def iterentries(&self) -> PyResult<Vec<PyTuple>> {
        let store = self.store(py);
        store.iter_py(py)
    }
});

py_class!(class historypackstore |py| {
    data store: Box<HistoryPackStore>;
    data path: PathBuf;

    def __new__(_cls, path: PyPathBuf, deletecorruptpacks: bool = false) -> PyResult<historypackstore> {
        let corruption_policy = if deletecorruptpacks {
            CorruptionPolicy::REMOVE
        } else {
            CorruptionPolicy::IGNORE
        };

        historypackstore::create_instance(py, Box::new(HistoryPackStore::new(path.as_path(), corruption_policy)), path.to_path_buf())
    }

    def getnodeinfo(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyTuple> {
        self.store(py).get_node_info_py(py, &name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        self.store(py).get_missing_py(py, &mut keys.iter(py)?)
    }

    def markforrefresh(&self) -> PyResult<PyObject> {
        self.store(py).force_rescan();
        Ok(Python::None(py))
    }

    def getmetrics(&self) -> PyResult<PyDict> {
        let (size, count) = match compute_store_size(self.path(py), vec!["histpack", "histidx"]) {
            Ok((size, count)) => (size, count),
            Err(_) => (0, 0),
        };

        let res = PyDict::new(py);
        res.set_item(py, "numpacks", count)?;
        res.set_item(py, "totalpacksize", size)?;
        Ok(res)
    }
});

py_class!(class indexedlogdatastore |py| {
    data store: Box<IndexedLogHgIdDataStore>;

    def __new__(_cls, path: &PyPath) -> PyResult<indexedlogdatastore> {
        indexedlogdatastore::create_instance(
            py,
            Box::new(IndexedLogHgIdDataStore::new(path.as_path()).map_pyerr(py)?),
        )
    }

    @staticmethod
    def repair(path: &PyPath) -> PyResult<Str> {
        py.allow_threads(|| IndexedLogHgIdDataStore::repair(path.as_path())).map_pyerr(py).map(Into::into)
    }

    def getdelta(&self, name: &PyPath, node: &PyBytes) -> PyResult<PyObject> {
        let store = self.store(py);
        store.get_delta_py(py, name, node)
    }

    def getdeltachain(&self, name: &PyPath, node: &PyBytes) -> PyResult<PyList> {
        let store = self.store(py);
        store.get_delta_chain_py(py, name, node)
    }

    def getmeta(&self, name: &PyPath, node: &PyBytes) -> PyResult<PyDict> {
        let store = self.store(py);
        store.get_meta_py(py, name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        let store = self.store(py);
        store.get_missing_py(py, &mut keys.iter(py)?)
    }

    def markforrefresh(&self) -> PyResult<PyObject> {
        let store = self.store(py);
        store.flush_py(py)?;
        Ok(Python::None(py))
    }

    def iterentries(&self) -> PyResult<Vec<PyTuple>> {
        let store = self.store(py);
        store.iter_py(py)
    }
});

py_class!(class indexedloghistorystore |py| {
    data store: Box<IndexedLogHgIdHistoryStore>;

    def __new__(_cls, path: &PyPath) -> PyResult<indexedloghistorystore> {
        indexedloghistorystore::create_instance(
            py,
            Box::new(IndexedLogHgIdHistoryStore::new(path.as_path()).map_pyerr(py)?),
        )
    }

    @staticmethod
    def repair(path: &PyPath) -> PyResult<PyUnicode> {
        IndexedLogHgIdHistoryStore::repair(path.as_path()).map_pyerr(py).map(|s| PyUnicode::new(py, &s))
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        let store = self.store(py);
        store.get_missing_py(py, &mut keys.iter(py)?)
    }

    def getnodeinfo(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyTuple> {
        let store = self.store(py);
        store.get_node_info_py(py, &name, node)
    }

    def markforrefresh(&self) -> PyResult<PyObject> {
        let store = self.store(py);
        store.flush_py(py)?;
        Ok(Python::None(py))
    }

    def iterentries(&self) -> PyResult<Vec<PyTuple>> {
        let store = self.store(py);
        store.iter_py(py)
    }
});

fn make_mutabledeltastore(
    packfilepath: Option<PyPathBuf>,
    indexedlogpath: Option<PyPathBuf>,
) -> Result<Arc<dyn HgIdMutableDeltaStore + Send>> {
    let store: Arc<dyn HgIdMutableDeltaStore + Send> = if let Some(packfilepath) = packfilepath {
        Arc::new(MutableDataPack::new(
            packfilepath.as_path(),
            DataPackVersion::One,
        )?)
    } else if let Some(indexedlogpath) = indexedlogpath {
        Arc::new(IndexedLogHgIdDataStore::new(indexedlogpath.as_path())?)
    } else {
        return Err(format_err!("Foo"));
    };
    Ok(store)
}

py_class!(pub class mutabledeltastore |py| {
    data store: Arc<dyn HgIdMutableDeltaStore>;

    def __new__(_cls, packfilepath: Option<PyPathBuf> = None, indexedlogpath: Option<PyPathBuf> = None) -> PyResult<mutabledeltastore> {
        let store = make_mutabledeltastore(packfilepath, indexedlogpath).map_pyerr(py)?;
        mutabledeltastore::create_instance(py, store)
    }

    def add(&self, name: PyPathBuf, node: &PyBytes, deltabasenode: &PyBytes, delta: &PyBytes, metadata: Option<PyDict> = None) -> PyResult<PyObject> {
        let store = self.store(py);
        store.add_py(py, &name, node, deltabasenode, delta, metadata)
    }

    def flush(&self) -> PyResult<Option<PyPathBuf>> {
        let store = self.store(py);
        store.flush_py(py)
    }

    def getdelta(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyObject> {
        let store = self.store(py);
        store.get_delta_py(py, &name, node)
    }

    def getdeltachain(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyList> {
        let store = self.store(py);
        store.get_delta_chain_py(py, &name, node)
    }

    def getmeta(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyDict> {
        let store = self.store(py);
        store.get_meta_py(py, &name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        let store = self.store(py);
        store.get_missing_py(py, &mut keys.iter(py)?)
    }
});

impl HgIdDataStore for mutabledeltastore {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).get(key)
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).get_delta(key)
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).get_delta_chain(key)
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).get_meta(key)
    }
}

impl LocalStore for mutabledeltastore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).get_missing(keys)
    }
}

impl HgIdMutableDeltaStore for mutabledeltastore {
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).add(delta, metadata)
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).flush()
    }
}

fn make_mutablehistorystore(
    packfilepath: Option<PyPathBuf>,
) -> Result<Arc<dyn HgIdMutableHistoryStore + Send>> {
    let store: Arc<dyn HgIdMutableHistoryStore + Send> = if let Some(packfilepath) = packfilepath {
        Arc::new(MutableHistoryPack::new(
            packfilepath.as_path(),
            HistoryPackVersion::One,
        )?)
    } else {
        return Err(format_err!("No packfile path passed in"));
    };

    Ok(store)
}

py_class!(pub class mutablehistorystore |py| {
    data store: Arc<dyn HgIdMutableHistoryStore>;

    def __new__(_cls, packfilepath: Option<PyPathBuf>) -> PyResult<mutablehistorystore> {
        let store = make_mutablehistorystore(packfilepath).map_pyerr(py)?;
        mutablehistorystore::create_instance(py, store)
    }

    def add(&self, name: PyPathBuf, node: &PyBytes, p1: &PyBytes, p2: &PyBytes, linknode: &PyBytes, copyfrom: Option<PyPathBuf>) -> PyResult<PyObject> {
        let store = self.store(py);
        store.add_py(py, &name, node, p1, p2, linknode, copyfrom.as_ref())
    }

    def flush(&self) -> PyResult<Option<PyPathBuf>> {
        let store = self.store(py);
        store.flush_py(py)
    }

    def getnodeinfo(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyTuple> {
        let store = self.store(py);
        store.get_node_info_py(py, &name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        let store = self.store(py);
        store.get_missing_py(py, &mut keys.iter(py)?)
    }
});

impl HgIdHistoryStore for mutablehistorystore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).get_node_info(key)
    }
}

impl LocalStore for mutablehistorystore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).get_missing(keys)
    }
}

impl HgIdMutableHistoryStore for mutablehistorystore {
    fn add(&self, key: &Key, info: &NodeInfo) -> Result<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).add(key, info)
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).flush()
    }
}

struct PyHgIdRemoteStoreInner {
    py_store: PyObject,
    datastore: Option<mutabledeltastore>,
    historystore: Option<mutablehistorystore>,
}

pub struct PyHgIdRemoteStore {
    inner: RwLock<PyHgIdRemoteStoreInner>,
}

impl PyHgIdRemoteStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let keys = keys
            .into_iter()
            .filter_map(|key| match key {
                StoreKey::HgId(key) => Some(from_key(py, &key)),
                StoreKey::Content(_, _) => None,
            })
            .collect::<Vec<_>>();

        if !keys.is_empty() {
            let inner = self.inner.read();
            inner
                .py_store
                .call_method(
                    py,
                    "prefetch",
                    (
                        inner.datastore.clone_ref(py),
                        inner.historystore.clone_ref(py),
                        keys,
                    ),
                    None,
                )
                .map_err(|e| PyErr::from(e))?;
        }
        Ok(())
    }
}

struct PyRemoteDataStore(Arc<PyHgIdRemoteStore>);
struct PyRemoteHistoryStore(Arc<PyHgIdRemoteStore>);

impl HgIdRemoteStore for PyHgIdRemoteStore {
    fn datastore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableDeltaStore>,
    ) -> Arc<dyn RemoteDataStore> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.inner.write().datastore = Some(mutabledeltastore::create_instance(py, store).unwrap());

        Arc::new(PyRemoteDataStore(self))
    }

    fn historystore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableHistoryStore>,
    ) -> Arc<dyn RemoteHistoryStore> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.inner.write().historystore =
            Some(mutablehistorystore::create_instance(py, store).unwrap());

        Arc::new(PyRemoteHistoryStore(self.clone()))
    }
}

impl RemoteDataStore for PyRemoteDataStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        self.0.prefetch(keys)
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }
}

impl HgIdDataStore for PyRemoteDataStore {
    fn get(&self, _key: &Key) -> Result<Option<Vec<u8>>> {
        unreachable!();
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        let missing = self.translate_lfs_missing(&[StoreKey::hgid(key.clone())])?;
        match self.prefetch(&missing) {
            Ok(()) => self
                .0
                .inner
                .read()
                .datastore
                .as_ref()
                .unwrap()
                .get_delta(key),
            Err(_) => Ok(None),
        }
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        let missing = self.translate_lfs_missing(&[StoreKey::hgid(key.clone())])?;
        match self.prefetch(&missing) {
            Ok(()) => self
                .0
                .inner
                .read()
                .datastore
                .as_ref()
                .unwrap()
                .get_delta_chain(key),
            Err(_) => Ok(None),
        }
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        let missing = self.translate_lfs_missing(&[StoreKey::hgid(key.clone())])?;
        match self.prefetch(&missing) {
            Ok(()) => self
                .0
                .inner
                .read()
                .datastore
                .as_ref()
                .unwrap()
                .get_meta(key),
            Err(_) => Ok(None),
        }
    }
}

impl LocalStore for PyRemoteDataStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.0
            .inner
            .read()
            .datastore
            .as_ref()
            .unwrap()
            .get_missing(keys)
    }
}

impl RemoteHistoryStore for PyRemoteHistoryStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        self.0.prefetch(keys)
    }
}

impl HgIdHistoryStore for PyRemoteHistoryStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        match self.prefetch(&[StoreKey::hgid(key.clone())]) {
            Ok(()) => self
                .0
                .inner
                .read()
                .historystore
                .as_ref()
                .unwrap()
                .get_node_info(key),
            Err(_) => Ok(None),
        }
    }
}

impl LocalStore for PyRemoteHistoryStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.0
            .inner
            .read()
            .historystore
            .as_ref()
            .unwrap()
            .get_missing(keys)
    }
}

py_class!(pub class pyremotestore |py| {
    data remote: Arc<PyHgIdRemoteStore>;

    def __new__(_cls, py_store: PyObject) -> PyResult<pyremotestore> {
        let store = Arc::new(PyHgIdRemoteStore { inner: RwLock::new(PyHgIdRemoteStoreInner { py_store, datastore: None, historystore: None }) });
        pyremotestore::create_instance(py, store)
    }
});

impl pyremotestore {
    fn into_inner(&self, py: Python) -> Arc<PyHgIdRemoteStore> {
        self.remote(py).clone()
    }
}

py_class!(pub class contentstore |py| {
    data store: Arc<ContentStore>;

    def __new__(_cls, path: Option<PyPathBuf>, config: config, remote: pyremotestore, memcache: Option<memcachestore>) -> PyResult<contentstore> {
        let remotestore = remote.into_inner(py);
        let config = config.get_cfg(py);

        let mut builder = ContentStoreBuilder::new(&config).remotestore(remotestore);

        builder = if let Some(path) = path {
            builder.local_path(path.as_path())
        } else {
            builder.no_local_store()
        };

        builder = if let Some(memcache) = memcache {
            builder.memcachestore(memcache.into_inner(py))
        } else {
            builder
        };

        let contentstore = builder.build().map_pyerr(py)?;
        contentstore::create_instance(py, Arc::new(contentstore))
    }

    def get(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyBytes> {
        let store = self.store(py);
        store.get_py(py, &name, node)
    }

    def getdelta(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyObject> {
        let store = self.store(py);
        store.get_delta_py(py, &name, node)
    }

    def getdeltachain(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyList> {
        let store = self.store(py);
        store.get_delta_chain_py(py, &name, node)
    }

    def getmeta(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyDict> {
        let store = self.store(py);
        store.get_meta_py(py, &name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        let store = self.store(py);
        store.get_missing_py(py, &mut keys.iter(py)?)
    }

    def add(&self, name: PyPathBuf, node: &PyBytes, deltabasenode: &PyBytes, delta: &PyBytes, metadata: Option<PyDict> = None) -> PyResult<PyObject> {
        let store = self.store(py);
        store.add_py(py, &name, node, deltabasenode, delta, metadata)
    }

    def flush(&self) -> PyResult<Option<PyPathBuf>> {
        let store = self.store(py);
        store.flush_py(py)
    }

    def prefetch(&self, keys: PyList) -> PyResult<PyObject> {
        let store = self.store(py);
        store.prefetch_py(py, keys)
    }

    def upload(&self, keys: PyList) -> PyResult<PyList> {
        let store = self.store(py);
        store.upload_py(py, keys)
    }

    def blob(&self, name: &PyPath, node: &PyBytes) -> PyResult<PyBytes> {
        let store = self.store(py);
        store.blob_py(py, name, node)
    }

    def metadata(&self, name: &PyPath, node: &PyBytes) -> PyResult<PyDict> {
        let store = self.store(py);
        store.metadata_py(py, name, node)
    }
});

impl contentstore {
    pub fn to_inner(&self, py: Python) -> Arc<ContentStore> {
        self.store(py).clone()
    }
}

py_class!(class metadatastore |py| {
    data store: Arc<MetadataStore>;

    def __new__(_cls, path: Option<PyPathBuf>, config: config, remote: pyremotestore, memcache: Option<memcachestore>) -> PyResult<metadatastore> {
        let remotestore = remote.into_inner(py);
        let config = config.get_cfg(py);

        let mut builder = MetadataStoreBuilder::new(&config).remotestore(remotestore);

        builder = if let Some(path) = path {
            builder.local_path(path.as_path())
        } else {
            builder.no_local_store()
        };

        builder = if let Some(memcache) = memcache {
            builder.memcachestore(memcache.into_inner(py))
        } else {
            builder
        };

        let metadatastore = Arc::new(builder.build().map_pyerr(py)?);
        metadatastore::create_instance(py, metadatastore)
    }

    def getnodeinfo(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyTuple> {
        self.store(py).get_node_info_py(py, &name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        self.store(py).get_missing_py(py, &mut keys.iter(py)?)
    }

    def add(&self, name: PyPathBuf, node: &PyBytes, p1: &PyBytes, p2: &PyBytes, linknode: &PyBytes, copyfrom: Option<PyPathBuf>) -> PyResult<PyObject> {
        let store = self.store(py);
        store.add_py(py, &name, node, p1, p2, linknode, copyfrom.as_ref())
    }

    def flush(&self) -> PyResult<Option<PyPathBuf>> {
        let store = self.store(py);
        store.flush_py(py)
    }

    def prefetch(&self, keys: PyList) -> PyResult<PyObject> {
        let store = self.store(py);
        store.prefetch_py(py, keys)
    }
});

impl metadatastore {
    pub fn to_inner(&self, py: Python) -> Arc<MetadataStore> {
        self.store(py).clone()
    }
}

py_class!(pub class memcachestore |py| {
    data memcache: Arc<MemcacheStore>;

    def __new__(_cls, config: config) -> PyResult<memcachestore> {
        let config = config.get_cfg(py);
        let memcache = Arc::new(MemcacheStore::new(&config).map_pyerr(py)?);
        memcachestore::create_instance(py, memcache)
    }
});

impl memcachestore {
    fn into_inner(&self, py: Python) -> Arc<MemcacheStore> {
        self.memcache(py).clone()
    }
}
