/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! revisionstore - Python interop layer for a Mercurial data and history store

#![allow(non_camel_case_types)]

use std::fs::read_dir;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::format_err;
use anyhow::Error;
use async_runtime::block_on;
use async_runtime::stream_to_iter as block_on_stream;
use configmodel::Config;
use configmodel::ConfigExt;
use cpython::*;
use cpython_ext::ExtractInner;
use cpython_ext::ExtractInnerRef;
use cpython_ext::PyErr;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use cpython_ext::Str;
use io::IO;
use parking_lot::RwLock;
use pyconfigloader::config;
use revisionstore::repack;
use revisionstore::scmstore::file_to_async_key_stream;
use revisionstore::scmstore::FetchMode;
use revisionstore::scmstore::FileAttributes;
use revisionstore::scmstore::FileStore;
use revisionstore::scmstore::FileStoreBuilder;
use revisionstore::scmstore::TreeStore;
use revisionstore::scmstore::TreeStoreBuilder;
use revisionstore::ContentStore;
use revisionstore::ContentStoreBuilder;
use revisionstore::CorruptionPolicy;
use revisionstore::DataPack;
use revisionstore::DataPackStore;
use revisionstore::DataPackVersion;
use revisionstore::Delta;
use revisionstore::EdenApiFileStore;
use revisionstore::EdenApiTreeStore;
use revisionstore::ExtStoredPolicy;
use revisionstore::HgIdDataStore;
use revisionstore::HgIdHistoryStore;
use revisionstore::HgIdMutableDeltaStore;
use revisionstore::HgIdMutableHistoryStore;
use revisionstore::HgIdRemoteStore;
use revisionstore::HistoryPack;
use revisionstore::HistoryPackStore;
use revisionstore::HistoryPackVersion;
use revisionstore::IndexedLogHgIdDataStore;
use revisionstore::IndexedLogHgIdDataStoreConfig;
use revisionstore::IndexedLogHgIdHistoryStore;
use revisionstore::LegacyStore;
use revisionstore::LocalStore;
use revisionstore::MemcacheStore;
use revisionstore::Metadata;
use revisionstore::MetadataStore;
use revisionstore::MetadataStoreBuilder;
use revisionstore::MutableDataPack;
use revisionstore::MutableHistoryPack;
use revisionstore::RemoteDataStore;
use revisionstore::RemoteHistoryStore;
use revisionstore::RepackKind;
use revisionstore::RepackLocation;
use revisionstore::StoreKey;
use revisionstore::StoreResult;
use revisionstore::StoreType;
use types::Key;
use types::NodeInfo;

use crate::datastorepyext::ContentDataStorePyExt;
use crate::datastorepyext::HgIdDataStorePyExt;
use crate::datastorepyext::HgIdMutableDeltaStorePyExt;
use crate::datastorepyext::IterableHgIdDataStorePyExt;
use crate::datastorepyext::RemoteDataStorePyExt;
use crate::historystorepyext::HgIdHistoryStorePyExt;
use crate::historystorepyext::HgIdMutableHistoryStorePyExt;
use crate::historystorepyext::IterableHgIdHistoryStorePyExt;
use crate::historystorepyext::RemoteHistoryStorePyExt;
use crate::pythonutil::from_key;
use crate::pythonutil::from_key_to_tuple;
use crate::pythonutil::from_tuple_to_key;

mod datastorepyext;
mod historystorepyext;
mod impl_into;
mod pythondatastore;
mod pythonfilescmstore;
mod pythonutil;

type Result<T, E = Error> = std::result::Result<T, E>;

pub use crate::pythondatastore::PythonHgIdDataStore;
use crate::pythonfilescmstore::PythonFileScmStore;
pub use crate::pythonutil::as_legacystore;

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
    m.add_class::<filescmstore>(py)?;
    m.add_class::<treescmstore>(py)?;
    m.add_class::<pyfilescmstore>(py)?;
    m.add(
        py,
        "repack",
        py_fn!(
            py,
            repack_py(
                packpath: &PyPath,
                stores: Option<(PyObject, metadatastore)>,
                full: bool,
                shared: bool,
                config: config
            )
        ),
    )?;
    m.add(
        py,
        "repair",
        py_fn!(
            py,
            repair(
                shared_path: &PyPath,
                local_path: Option<&PyPath>,
                suffix: Option<&PyPath>,
                config: config
            )
        ),
    )?;

    impl_into::register(py);
    Ok(m)
}

fn repack_py(
    py: Python,
    packpath: &PyPath,
    stores: Option<(PyObject, metadatastore)>,
    full: bool,
    shared: bool,
    config: config,
) -> PyResult<PyNone> {
    let stores = if let Some((content, metadata)) = stores {
        Some((as_legacystore(py, content)?, metadata.extract_inner(py)))
    } else {
        None
    };

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

    repack(
        packpath.to_path_buf(),
        stores,
        kind,
        location,
        &config.get_cfg(py),
    )
    .map_pyerr(py)?;

    Ok(PyNone)
}

fn repair(
    py: Python,
    shared_path: &PyPath,
    local_path: Option<&PyPath>,
    suffix: Option<&PyPath>,
    config: config,
) -> PyResult<Str> {
    let config = config.get_cfg(py);
    py.allow_threads::<Result<String>, _>(|| {
        let mut message = ContentStore::repair(
            shared_path.as_path(),
            local_path.map(|p| p.as_path()),
            suffix.map(|p| p.as_path()),
            &config,
        )?;
        message.push_str(
            MetadataStore::repair(
                shared_path.as_path(),
                local_path.map(|p| p.as_path()),
                suffix.map(|p| p.as_path()),
                &config,
            )?
            .as_str(),
        );
        Ok(message)
    })
    .map_pyerr(py)
    .map(Into::into)
}

py_class!(class datapack |py| {
    data store: Box<DataPack>;

    def __new__(
        _cls,
        path: &PyPath
    ) -> PyResult<datapack> {
        datapack::create_instance(
            py,
            Box::new(DataPack::new(path, ExtStoredPolicy::Ignore).map_pyerr(py)?),
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

    def __new__(_cls, path: &PyPath, deletecorruptpacks: bool = false, maxbytes: Option<u64> = None) -> PyResult<datapackstore> {
        let corruption_policy = if deletecorruptpacks {
            CorruptionPolicy::REMOVE
        } else {
            CorruptionPolicy::IGNORE
        };

        datapackstore::create_instance(py, Box::new(DataPackStore::new(path, corruption_policy, maxbytes, ExtStoredPolicy::Ignore)), path.to_path_buf())
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

    def __new__(_cls, path: PyPathBuf, deletecorruptpacks: bool = false, maxbytes: Option<u64> = None) -> PyResult<historypackstore> {
        let corruption_policy = if deletecorruptpacks {
            CorruptionPolicy::REMOVE
        } else {
            CorruptionPolicy::IGNORE
        };

        historypackstore::create_instance(py, Box::new(HistoryPackStore::new(path.as_path(), corruption_policy, maxbytes)), path.to_path_buf())
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
        let config = IndexedLogHgIdDataStoreConfig { max_log_count: None, max_bytes_per_log: None, max_bytes: None };
        indexedlogdatastore::create_instance(
            py,
            Box::new(IndexedLogHgIdDataStore::new(path.as_path(), ExtStoredPolicy::Ignore, &config, StoreType::Local).map_pyerr(py)?),
        )
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

    def __new__(_cls, path: &PyPath, config: config) -> PyResult<indexedloghistorystore> {
        let config = config.get_cfg(py);
        indexedloghistorystore::create_instance(
            py,
            Box::new(IndexedLogHgIdHistoryStore::new(path.as_path(), &config, StoreType::Local).map_pyerr(py)?),
        )
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
        ))
    } else if let Some(indexedlogpath) = indexedlogpath {
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        Arc::new(IndexedLogHgIdDataStore::new(
            indexedlogpath.as_path(),
            ExtStoredPolicy::Ignore,
            &config,
            StoreType::Local,
        )?)
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

    def flush(&self) -> PyResult<Option<Vec<PyPathBuf>>> {
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

impl ExtractInnerRef for mutabledeltastore {
    type Inner = Arc<dyn HgIdMutableDeltaStore>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.store(py)
    }
}

impl HgIdDataStore for mutabledeltastore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).get(key)
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).get_meta(key)
    }

    fn refresh(&self) -> Result<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).refresh()
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

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
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
        ))
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

    def flush(&self) -> PyResult<Option<Vec<PyPathBuf>>> {
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

impl ExtractInnerRef for mutablehistorystore {
    type Inner = Arc<dyn HgIdMutableHistoryStore>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.store(py)
    }
}

impl HgIdHistoryStore for mutablehistorystore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).get_node_info(key)
    }

    fn refresh(&self) -> Result<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        self.store(py).refresh()
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

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
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
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.0.prefetch(keys)?;
        self.get_missing(keys)
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }
}

impl HgIdDataStore for PyRemoteDataStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        self.prefetch(&[key.clone()])?;
        self.0.inner.read().datastore.as_ref().unwrap().get(key)
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        match self.prefetch(&[key.clone()]) {
            Ok(_) => self
                .0
                .inner
                .read()
                .datastore
                .as_ref()
                .unwrap()
                .get_meta(key),
            Err(_) => Ok(StoreResult::NotFound(key)),
        }
    }

    fn refresh(&self) -> Result<()> {
        Ok(())
    }
}

impl LocalStore for PyRemoteDataStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
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

    fn refresh(&self) -> Result<()> {
        Ok(())
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

impl ExtractInnerRef for pyremotestore {
    type Inner = Arc<PyHgIdRemoteStore>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.remote(py)
    }
}

// Python wrapper around an EdenAPI-backed remote store for files.
//
// This type exists for the sole purpose of allowing an `EdenApiFileStore`
// to be passed from Rust to Python and back into Rust. It cannot be created
// by Python code and does not expose any functionality to Python.
py_class!(pub class edenapifilestore |py| {
    data remote: Arc<EdenApiFileStore>;
});

impl edenapifilestore {
    pub fn new(py: Python, remote: Arc<EdenApiFileStore>) -> PyResult<Self> {
        edenapifilestore::create_instance(py, remote)
    }
}

impl ExtractInnerRef for edenapifilestore {
    type Inner = Arc<EdenApiFileStore>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.remote(py)
    }
}

// Python wrapper around an EdenAPI-backed remote store for trees.
//
// This type exists for the sole purpose of allowing an `EdenApiTreeStore`
// to be passed from Rust to Python and back into Rust. It cannot be created
// by Python code and does not expose any functionality to Python.
py_class!(pub class edenapitreestore |py| {
    data remote: Arc<EdenApiTreeStore>;
});

impl edenapitreestore {
    pub fn new(py: Python, remote: Arc<EdenApiTreeStore>) -> PyResult<Self> {
        edenapitreestore::create_instance(py, remote)
    }
}

impl ExtractInnerRef for edenapitreestore {
    type Inner = Arc<EdenApiTreeStore>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.remote(py)
    }
}

py_class!(pub class contentstore |py| {
    data store: Arc<ContentStore>;

    def __new__(_cls,
        path: Option<PyPathBuf>,
        config: config,
        remote: pyremotestore,
        memcache: Option<memcachestore>,
        edenapi: Option<edenapifilestore> = None,
        suffix: Option<String> = None,
        correlator: Option<String> = None
    ) -> PyResult<contentstore> {
        let remotestore = remote.extract_inner(py);
        let config = config.get_cfg(py);

        let mut builder = ContentStoreBuilder::new(&config).correlator(correlator);

        builder = if let Some(edenapi) = edenapi {
            builder.remotestore(edenapi.extract_inner(py))
        } else {
            builder.remotestore(remotestore)
        };

        builder = if let Some(path) = path {
            builder.local_path(path.as_path())
        } else {
            builder.no_local_store()
        };

        builder = if let Some(memcache) = memcache {
            builder.memcachestore(memcache.extract_inner(py))
        } else {
            builder
        };

        builder = if let Some(suffix) = suffix {
            builder.suffix(suffix)
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

    def flush(&self) -> PyResult<Option<Vec<PyPathBuf>>> {
        let store = self.store(py);
        store.flush_py(py)
    }

    def prefetch(&self, keys: PyList) -> PyResult<PyObject> {
        let store = self.store(py);
        store.prefetch_py(py, keys)
    }

    def markforrefresh(&self) -> PyResult<PyNone> {
        let store = self.store(py);
        store.refresh_py(py)
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

    def getloggedfetches(&self) -> PyResult<Vec<PyPathBuf>> {
        let store = self.store(py);
        Ok(store.get_logged_fetches().into_iter().map(|p| p.into()).collect::<Vec<PyPathBuf>>())
    }

    def getsharedmutable(&self) -> PyResult<mutabledeltastore> {
        let store = self.store(py);
        mutabledeltastore::create_instance(py, store.get_shared_mutable())
    }
});

impl ExtractInnerRef for contentstore {
    type Inner = Arc<ContentStore>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.store(py)
    }
}

py_class!(class metadatastore |py| {
    data store: Arc<MetadataStore>;

    def __new__(_cls,
        path: Option<PyPathBuf>,
        config: config,
        remote: pyremotestore,
        memcache: Option<memcachestore>,
        edenapi: Option<edenapifilestore> = None,
        suffix: Option<String> = None
    ) -> PyResult<metadatastore> {
        let remotestore = remote.extract_inner(py);
        let config = config.get_cfg(py);

        let mut builder = MetadataStoreBuilder::new(&config);

        builder = if let Some(edenapi) = edenapi {
            builder.remotestore(edenapi.extract_inner(py))
        } else {
            builder.remotestore(remotestore)
        };

        builder = if let Some(path) = path {
            builder.local_path(path.as_path())
        } else {
            builder.no_local_store()
        };

        builder = if let Some(memcache) = memcache {
            builder.memcachestore(memcache.extract_inner(py))
        } else {
            builder
        };

        builder = if let Some(suffix) = suffix {
            builder.suffix(suffix)
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

    def flush(&self) -> PyResult<Option<Vec<PyPathBuf>>> {
        let store = self.store(py);
        store.flush_py(py)
    }

    def prefetch(&self, keys: PyList) -> PyResult<PyObject> {
        let store = self.store(py);
        store.prefetch_py(py, keys)
    }

    def markforrefresh(&self) -> PyResult<PyNone> {
        let store = self.store(py);
        store.refresh_py(py)
    }

    def getsharedmutable(&self) -> PyResult<mutablehistorystore> {
        let store = self.store(py);
        mutablehistorystore::create_instance(py, store.get_shared_mutable())
    }
});

impl ExtractInnerRef for metadatastore {
    type Inner = Arc<MetadataStore>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.store(py)
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

impl ExtractInnerRef for memcachestore {
    type Inner = Arc<MemcacheStore>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.memcache(py)
    }
}

// TODO(meyer): Make this a `BoxedRwStore` (and introduce such a concept). Will need to implement write
// for FallbackStore.
/// Construct a file ReadStore using the provided config, optionally falling back
/// to the provided legacy HgIdDataStore.
fn make_filescmstore<'a>(
    path: Option<&'a Path>,
    config: &'a dyn Config,
    remote: Arc<PyHgIdRemoteStore>,
    memcache: Option<Arc<MemcacheStore>>,
    edenapi_filestore: Option<Arc<EdenApiFileStore>>,
    suffix: Option<String>,
    correlator: Option<String>,
) -> Result<(Arc<FileStore>, Arc<ContentStore>)> {
    let mut builder = ContentStoreBuilder::new(&config).correlator(correlator.clone());
    let mut filestore_builder = FileStoreBuilder::new(&config);

    if config.get_or_default::<bool>("scmstore", "auxindexedlog")? {
        filestore_builder = filestore_builder.store_aux_data();
    }

    if let Some(correlator) = correlator {
        filestore_builder = filestore_builder.correlator(correlator);
    }

    builder = if let Some(path) = path {
        filestore_builder = filestore_builder.local_path(path);
        builder.local_path(path)
    } else {
        builder.no_local_store()
    };

    if let Some(memcache) = memcache {
        filestore_builder = filestore_builder.memcache(memcache.clone());
        builder = builder.memcachestore(memcache)
    };

    if let Some(ref suffix) = suffix {
        builder = builder.suffix(suffix);
        filestore_builder = filestore_builder.suffix(suffix);
    }

    builder = if let Some(edenapi) = edenapi_filestore {
        filestore_builder = filestore_builder.edenapi(edenapi.clone());
        builder.remotestore(edenapi)
    } else {
        builder.remotestore(remote)
    };

    let indexedlog_local = filestore_builder.build_indexedlog_local()?;
    let indexedlog_cache = filestore_builder.build_indexedlog_cache()?;
    let lfs_local = filestore_builder.build_lfs_local()?;
    let lfs_cache = filestore_builder.build_lfs_cache()?;

    if let Some(indexedlog_local) = indexedlog_local {
        filestore_builder = filestore_builder.indexedlog_local(indexedlog_local.clone());
        builder = builder.shared_indexedlog_local(indexedlog_local);
    }

    if let Some(ref cache) = indexedlog_cache {
        filestore_builder = filestore_builder.indexedlog_cache(cache.clone());
        builder = builder.shared_indexedlog_shared(cache.clone());
    }

    if let Some(lfs_local) = lfs_local {
        filestore_builder = filestore_builder.lfs_local(lfs_local.clone());
        builder = builder.shared_lfs_local(lfs_local);
    }

    if let Some(lfs_cache) = lfs_cache {
        filestore_builder = filestore_builder.lfs_cache(lfs_cache.clone());
        builder = builder.shared_lfs_shared(lfs_cache);
    }

    let contentstore = Arc::new(builder.build()?);

    filestore_builder = filestore_builder.contentstore(contentstore.clone());

    let filestore = Arc::new(filestore_builder.build()?);

    Ok((filestore, contentstore))
}

py_class!(pub class filescmstore |py| {
    data store: Arc<FileStore>;
    data contentstore: Arc<ContentStore>;

    def __new__(_cls,
        path: Option<PyPathBuf>,
        config: config,
        remote: pyremotestore,
        memcache: Option<memcachestore>,
        edenapi: Option<edenapifilestore> = None,
        suffix: Option<String> = None,
        correlator: Option<String> = None
    ) -> PyResult<filescmstore> {
        // Extract Rust Values
        let path = path.as_ref().map(|v| v.as_path());
        let config = config.get_cfg(py);
        let remote = remote.extract_inner(py);
        let memcache = memcache.map(|v| v.extract_inner(py));
        let edenapi = edenapi.map(|v| v.extract_inner(py));

        let (filestore, contentstore) = make_filescmstore(path, &config, remote, memcache, edenapi, suffix, correlator).map_pyerr(py)?;

        filescmstore::create_instance(py, filestore, contentstore)
    }

    def get_contentstore(&self) -> PyResult<contentstore> {
        contentstore::create_instance(py, self.contentstore(py).clone())
    }

    def test_fetch(&self, path: PyPathBuf) -> PyResult<PyNone> {
        let keys: Vec<_> = block_on_stream(block_on(file_to_async_key_stream(path.to_path_buf())).map_pyerr(py)?).collect();
        let fetch_result = self.store(py).fetch(keys.into_iter(), FileAttributes { content: true, aux_data: true }, FetchMode::AllowRemote );

        let io = IO::main().map_pyerr(py)?;
        let mut stdout = io.output();

        let (found, missing, _errors) = fetch_result.consume();

        for (_, file) in found.into_iter() {
            write!(stdout, "Successfully fetched file: {:#?}\n", file).map_pyerr(py)?;
        }
        for (key, _) in missing.into_iter() {
            write!(stdout, "Failed to fetch file: {:#?}\n", key).map_pyerr(py)?;
        }

        Ok(PyNone)
    }

    def fetch_contentsha256(&self, keys: PyList) -> PyResult<PyList> {
        let keys = keys
            .iter(py)
            .map(|tuple| from_tuple_to_key(py, &tuple))
            .collect::<PyResult<Vec<Key>>>()?;
        let results = PyList::new(py, &[]);
        let fetch_result = self.store(py).fetch(keys.into_iter(), FileAttributes { content: false, aux_data: true}, FetchMode::AllowRemote );

        let (found, missing, _errors) = fetch_result.consume();
        // TODO(meyer): FileStoreFetch should have utility methods to various consumer cases like this (get complete, get missing, transform to Result<EntireBatch>, transform to iterator of Result<IndividualFetch>, etc)
        // For now we just error with the first incomplete key, passing on the last recorded error if any are available.
        if let Some((key, mut errors)) = missing.into_iter().next() {
            if let Some(err) = errors.pop() {
                return Err(err.context(format!("failed to fetch {}, received error", key))).map_pyerr(py);
            } else {
                return Err(format_err!("failed to fetch {}", key)).map_pyerr(py);
            }
        }
        for (key, storefile) in found.into_iter() {
            let key_tuple = from_key_to_tuple(py, &key).into_object();
            let content_sha256 = storefile.aux_data().map_pyerr(py)?.content_sha256;
            let content_sha256 = PyBytes::new(py, &content_sha256.into_inner());
            let result_tuple = PyTuple::new(
                py,
                &[
                    key_tuple,
                    content_sha256.into_object(),
                ],
            );
            results.append(py, result_tuple.into_object());
        }
        Ok(results)
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

    def flush(&self) -> PyResult<Option<Vec<PyPathBuf>>> {
        let store = self.store(py);
        store.flush_py(py)
    }

    def prefetch(&self, keys: PyList) -> PyResult<PyObject> {
        let store = self.store(py);
        store.prefetch_py(py, keys)
    }

    def markforrefresh(&self) -> PyResult<PyNone> {
        let store = self.store(py);
        store.refresh_py(py)
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

    def getloggedfetches(&self) -> PyResult<Vec<PyPathBuf>> {
        let store = self.store(py);
        Ok(store.get_logged_fetches().into_iter().map(|p| p.into()).collect::<Vec<PyPathBuf>>())
    }

    def getmetrics(&self) -> PyResult<Vec<PyTuple>> {
        let store = self.store(py);
        Ok(store.metrics().into_iter().map(|(k, v)| {
            PyTuple::new(
                py,
                &[
                    k.to_py_object(py).into_object(),
                    v.to_py_object(py).into_object(),
                ],
            )
        }).collect::<Vec<PyTuple>>())
    }

    def getsharedmutable(&self) -> PyResult<mutabledeltastore> {
        let store = self.store(py);
        mutabledeltastore::create_instance(py, store.get_shared_mutable())
    }
});

impl ExtractInnerRef for filescmstore {
    type Inner = Arc<FileStore>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.store(py)
    }
}

// TODO(meyer): Make this a `BoxedRwStore` (and introduce such a concept). Will need to implement write
// for FallbackStore.
/// Construct a tree ReadStore using the provided config, optionally falling back
/// to the provided legacy HgIdDataStore.
fn make_treescmstore<'a>(
    path: Option<&'a Path>,
    config: &'a dyn Config,
    remote: Arc<PyHgIdRemoteStore>,
    memcache: Option<Arc<MemcacheStore>>,
    edenapi_treestore: Option<Arc<EdenApiTreeStore>>,
    filestore: Option<Arc<FileStore>>,
    suffix: Option<String>,
    correlator: Option<String>,
) -> Result<(Arc<TreeStore>, Arc<ContentStore>)> {
    let mut builder = ContentStoreBuilder::new(&config).correlator(correlator);
    let mut treestore_builder = TreeStoreBuilder::new(&config);

    builder = if let Some(path) = path {
        treestore_builder = treestore_builder.local_path(path);
        builder.local_path(path)
    } else {
        builder.no_local_store()
    };

    if let Some(memcache) = memcache {
        builder = builder.memcachestore(memcache.clone());
        treestore_builder = treestore_builder.memcache(memcache);
    };

    if let Some(ref suffix) = suffix {
        builder = builder.suffix(suffix);
        treestore_builder = treestore_builder.suffix(suffix);
    }

    // Extract EdenApiAdapter for scmstore construction later on
    builder = if let Some(edenapi) = edenapi_treestore {
        treestore_builder = treestore_builder.edenapi(edenapi.clone());
        builder.remotestore(edenapi)
    } else {
        builder.remotestore(remote)
    };

    if let Some(filestore) = filestore {
        treestore_builder = treestore_builder.filestore(filestore);
    }

    let indexedlog_local = treestore_builder.build_indexedlog_local()?;
    let indexedlog_cache = treestore_builder.build_indexedlog_cache()?;

    if let Some(indexedlog_local) = indexedlog_local {
        treestore_builder = treestore_builder.indexedlog_local(indexedlog_local.clone());
        builder = builder.shared_indexedlog_local(indexedlog_local);
    }

    if let Some(ref cache) = indexedlog_cache {
        treestore_builder = treestore_builder.indexedlog_cache(cache.clone());
        builder = builder.shared_indexedlog_shared(cache.clone());
    }

    let contentstore = Arc::new(builder.build()?);

    treestore_builder = treestore_builder.contentstore(contentstore.clone());

    let treestore = Arc::new(treestore_builder.build()?);

    Ok((treestore, contentstore))
}

py_class!(pub class pyfilescmstore |py| {
    data inner: Arc<PythonFileScmStore>;

    def __new__(_cls,
        py_store: PyObject,
    ) -> PyResult<pyfilescmstore> {
        pyfilescmstore::create_instance(py, Arc::new(PythonFileScmStore::new(py_store)))
    }
});

impl ExtractInnerRef for pyfilescmstore {
    type Inner = Arc<PythonFileScmStore>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.inner(py)
    }
}

py_class!(pub class treescmstore |py| {
    data store: Arc<TreeStore>;
    data contentstore: Arc<ContentStore>;

    def __new__(_cls,
        path: Option<PyPathBuf>,
        config: config,
        remote: pyremotestore,
        memcache: Option<memcachestore>,
        edenapi: Option<edenapitreestore> = None,
        filestore: Option<filescmstore> = None,
        suffix: Option<String> = None,
        correlator: Option<String> = None
    ) -> PyResult<treescmstore> {
        // Extract Rust Values
        let path = path.as_ref().map(|v| v.as_path());
        let config = config.get_cfg(py);
        let remote = remote.extract_inner(py);
        let memcache = memcache.map(|v| v.extract_inner(py));
        let edenapi = edenapi.map(|v| v.extract_inner(py));
        let filestore = filestore.map(|v| v.extract_inner(py));

        let (treestore, contentstore) = make_treescmstore(path, &config, remote, memcache, edenapi, filestore, suffix, correlator).map_pyerr(py)?;

        treescmstore::create_instance(py, treestore, contentstore)
    }

    def get_contentstore(&self) -> PyResult<contentstore> {
        contentstore::create_instance(py, self.contentstore(py).clone())
    }

    def test_fetch(&self, path: PyPathBuf) -> PyResult<PyNone> {
        let keys: Vec<_> = block_on_stream(block_on(file_to_async_key_stream(path.to_path_buf())).map_pyerr(py)?).collect();
        let fetch_result = self.store(py).fetch_batch(keys.into_iter(), FetchMode::AllowRemote);

        let io = IO::main().map_pyerr(py)?;
        let mut stdout = io.output();

        let (found, missing, _errors) = fetch_result.consume();
        for complete in found.into_iter() {
            write!(stdout, "Successfully fetched tree: {:#?}\n", complete).map_pyerr(py)?;
        }
        for incomplete in missing.into_iter() {
            write!(stdout, "Failed to fetch tree: {:#?}\n", incomplete).map_pyerr(py)?;
        }

        Ok(PyNone)
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

    def flush(&self) -> PyResult<Option<Vec<PyPathBuf>>> {
        let store = self.store(py);
        store.flush_py(py)
    }

    def prefetch(&self, keys: PyList) -> PyResult<PyObject> {
        let store = self.store(py);
        store.prefetch_py(py, keys)
    }

    def markforrefresh(&self) -> PyResult<PyNone> {
        let store = self.store(py);
        store.refresh_py(py)
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

    def getloggedfetches(&self) -> PyResult<Vec<PyPathBuf>> {
        let _ = py;
        // TODO(meyer): Make sure we're only supposed to be tracking fetches for files, not trees.
        Ok(Vec::new())
    }

    def getsharedmutable(&self) -> PyResult<mutabledeltastore> {
        let store = self.store(py);
        mutabledeltastore::create_instance(py, store.get_shared_mutable())
    }
});

impl ExtractInnerRef for treescmstore {
    type Inner = Arc<TreeStore>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.store(py)
    }
}
