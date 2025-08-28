/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! revisionstore - Python interop layer for a Mercurial data and history store

#![allow(non_camel_case_types)]

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Error;
use anyhow::anyhow;
use configmodel::Config;
use cpython::*;
use cpython_ext::ExtractInner;
use cpython_ext::ExtractInnerRef;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use pyconfigloader::config;
use pythonutil::key_error;
use pythonutil::to_key;
use pythonutil::to_node;
use revisionstore::ContentHash;
use revisionstore::Delta;
use revisionstore::HgIdDataStore;
use revisionstore::HgIdHistoryStore;
use revisionstore::HgIdMutableDeltaStore;
use revisionstore::HgIdMutableHistoryStore;
use revisionstore::HistoryStore;
use revisionstore::IndexedLogHgIdDataStore;
use revisionstore::IndexedLogHgIdDataStoreConfig;
use revisionstore::IndexedLogHgIdHistoryStore;
use revisionstore::LocalStore;
use revisionstore::Metadata;
use revisionstore::MetadataStore;
use revisionstore::MetadataStoreBuilder;
use revisionstore::SaplingRemoteApiFileStore;
use revisionstore::SaplingRemoteApiTreeStore;
use revisionstore::StoreKey;
use revisionstore::StoreResult;
use revisionstore::StoreType;
use revisionstore::scmstore::FileAttributes;
use revisionstore::scmstore::FileStore;
use revisionstore::scmstore::TreeStore;
use revisionstore::scmstore::TreeStoreBuilder;
use storemodel::SerializationFormat;
use types::FetchContext;
use types::Key;
use types::NodeInfo;

use crate::datastorepyext::HgIdDataStorePyExt;
use crate::datastorepyext::HgIdMutableDeltaStorePyExt;
use crate::datastorepyext::IterableHgIdDataStorePyExt;
use crate::historystorepyext::HgIdHistoryStorePyExt;
use crate::historystorepyext::HgIdMutableHistoryStorePyExt;
use crate::historystorepyext::IterableHgIdHistoryStorePyExt;
use crate::historystorepyext::RemoteHistoryStorePyExt;
use crate::pythonutil::from_key_to_tuple;
use crate::pythonutil::from_tuple_to_key;

mod datastorepyext;
mod historystorepyext;
mod impl_into;
mod pythondatastore;
mod pythonutil;

type Result<T, E = Error> = std::result::Result<T, E>;

pub use crate::pythondatastore::PythonHgIdDataStore;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "revisionstore"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<indexedlogdatastore>(py)?;
    m.add_class::<indexedloghistorystore>(py)?;
    m.add_class::<mutabledeltastore>(py)?;
    m.add_class::<mutablehistorystore>(py)?;
    m.add_class::<metadatastore>(py)?;
    m.add_class::<filescmstore>(py)?;
    m.add_class::<treescmstore>(py)?;
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

fn repair(
    py: Python,
    shared_path: &PyPath,
    local_path: Option<&PyPath>,
    suffix: Option<&PyPath>,
    config: config,
) -> PyResult<String> {
    let config = config.get_cfg(py);
    py.allow_threads::<Result<String>, _>(|| {
        let mut message = revisionstore::repair(
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
}

py_class!(class indexedlogdatastore |py| {
    data store: Box<IndexedLogHgIdDataStore>;

    def __new__(_cls, path: &PyPath) -> PyResult<indexedlogdatastore> {
        let config = IndexedLogHgIdDataStoreConfig { max_log_count: None, max_bytes_per_log: None, max_bytes: None, btrfs_compression: false };
        indexedlogdatastore::create_instance(
            py,
            Box::new(IndexedLogHgIdDataStore::new(
                &BTreeMap::<&str, &str>::new(),
                path.as_path(),
                &config,
                StoreType::Permanent,
                // TODO: allow specifying Git format
                SerializationFormat::Hg,
            ).map_pyerr(py)?),
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
            Box::new(IndexedLogHgIdHistoryStore::new(path.as_path(), &config, StoreType::Permanent).map_pyerr(py)?),
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
    indexedlogpath: PyPathBuf,
) -> Result<Arc<dyn HgIdMutableDeltaStore + Send>> {
    let config = IndexedLogHgIdDataStoreConfig {
        max_log_count: None,
        max_bytes_per_log: None,
        max_bytes: None,
        btrfs_compression: false,
    };
    Ok(Arc::new(IndexedLogHgIdDataStore::new(
        &BTreeMap::<&str, &str>::new(),
        indexedlogpath.as_path(),
        &config,
        StoreType::Permanent,
        // TODO: allow specifying Git format
        SerializationFormat::Hg,
    )?))
}

py_class!(pub class mutabledeltastore |py| {
    data store: Arc<dyn HgIdMutableDeltaStore>;

    def __new__(_cls, indexedlogpath: PyPathBuf) -> PyResult<mutabledeltastore> {
        let store = make_mutabledeltastore(indexedlogpath).map_pyerr(py)?;
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
    indexedlogpath: PyPathBuf,
) -> Result<Arc<dyn HgIdMutableHistoryStore + Send>> {
    Ok(Arc::new(IndexedLogHgIdHistoryStore::new(
        indexedlogpath.as_path(),
        &BTreeMap::<&str, &str>::new(),
        StoreType::Permanent,
    )?))
}

py_class!(pub class mutablehistorystore |py| {
    data store: Arc<dyn HgIdMutableHistoryStore>;

    def __new__(_cls, indexedlogpath: PyPathBuf) -> PyResult<mutablehistorystore> {
        let store = make_mutablehistorystore(indexedlogpath).map_pyerr(py)?;
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

// Python wrapper around an SaplingRemoteAPI-backed remote store for files.
//
// This type exists for the sole purpose of allowing an `SaplingRemoteApiFileStore`
// to be passed from Rust to Python and back into Rust. It cannot be created
// by Python code and does not expose any functionality to Python.
py_class!(pub class edenapifilestore |py| {
    data remote: Arc<SaplingRemoteApiFileStore>;
});

impl edenapifilestore {
    pub fn new(py: Python, remote: Arc<SaplingRemoteApiFileStore>) -> PyResult<Self> {
        edenapifilestore::create_instance(py, remote)
    }
}

impl ExtractInnerRef for edenapifilestore {
    type Inner = Arc<SaplingRemoteApiFileStore>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.remote(py)
    }
}

// Python wrapper around an SaplingRemoteAPI-backed remote store for trees.
//
// This type exists for the sole purpose of allowing an `SaplingRemoteApiTreeStore`
// to be passed from Rust to Python and back into Rust. It cannot be created
// by Python code and does not expose any functionality to Python.
py_class!(pub class edenapitreestore |py| {
    data remote: Arc<SaplingRemoteApiTreeStore>;
});

impl edenapitreestore {
    pub fn new(py: Python, remote: Arc<SaplingRemoteApiTreeStore>) -> PyResult<Self> {
        edenapitreestore::create_instance(py, remote)
    }
}

impl ExtractInnerRef for edenapitreestore {
    type Inner = Arc<SaplingRemoteApiTreeStore>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.remote(py)
    }
}

py_class!(class metadatastore |py| {
    data store: Arc<dyn HistoryStore>;

    def __new__(_cls,
        path: Option<PyPathBuf>,
        config: config,
        edenapi: Option<edenapifilestore>,
        suffix: Option<String> = None
    ) -> PyResult<metadatastore> {
        let config = config.get_cfg(py);

        let mut builder = MetadataStoreBuilder::new(&config);

        builder = if let Some(edenapi) = edenapi {
            builder.remotestore(edenapi.extract_inner(py))
        } else {
            builder
        };

        builder = if let Some(path) = path {
            builder.local_path(path.as_path())
        } else {
            builder.no_local_store()
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

    def getlocalnodeinfo(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<Option<PyTuple>> {
        self.store(py).get_local_node_info_py(py, &name, node)
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

    def prefetch(&self, keys: PyList, length: Option<u32> = None) -> PyResult<PyObject> {
        let store = self.store(py);
        store.prefetch_py(py, keys, length)
    }

    def markforrefresh(&self) -> PyResult<PyNone> {
        let store = self.store(py);
        store.refresh_py(py)
    }

    def getsharedmutable(&self) -> PyResult<Self> {
        let store = self.store(py);
        Self::create_instance(py, store.with_shared_only())
    }
});

py_class!(pub class filescmstore |py| {
    data store: Arc<FileStore>;

    def fetch_content_blake3(&self, keys: PyList) -> PyResult<PyList> {
        let keys = keys
            .iter(py)
            .map(|tuple| from_tuple_to_key(py, &tuple))
            .collect::<PyResult<Vec<Key>>>()?;
        let results = PyList::new(py, &[]);
        let fetch_result = self.store(py).fetch(FetchContext::default(), keys.into_iter(), FileAttributes::AUX);

        let (found, missing, _errors) = fetch_result.consume();
        // TODO(meyer): FileStoreFetch should have utility methods to various consumer cases like this (get complete, get missing, transform to Result<EntireBatch>, transform to iterator of Result<IndividualFetch>, etc)
        // For now we just error with the first incomplete key, passing on the last recorded error if any are available.
        if let Some((key, err)) = missing.into_iter().next() {
            return Err(err.context(format!("failed to fetch {}, received error", key))).map_pyerr(py);
        }
        for (key, storefile) in found.into_iter() {
            let key_tuple = from_key_to_tuple(py, &key).into_object();
            let content_blake3 = storefile.aux_data().map_pyerr(py)?.blake3;
            let content_blake3 = PyBytes::new(py, content_blake3.as_ref());
            let result_tuple = PyTuple::new(
                py,
                &[
                    key_tuple,
                    content_blake3.into_object(),
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

        let keys = keys
            .iter(py)
            .map(|tuple| from_tuple_to_key(py, &tuple))
            .collect::<PyResult<Vec<Key>>>()?;
        py.allow_threads(|| FileStore::prefetch(store, keys)).map_pyerr(py)?;

        Ok(Python::None(py))
    }

    def markforrefresh(&self) -> PyResult<PyNone> {
        let store = self.store(py);
        store.refresh_py(py)
    }

    def upload_lfs(&self, keys: PyList) -> PyResult<PyList> {
        let keys = keys
            .iter(py)
            .map(|tuple| Ok(StoreKey::from(from_tuple_to_key(py, &tuple)?)))
            .collect::<PyResult<Vec<StoreKey>>>()?;
        let not_uploaded = self.store(py).upload_lfs(&keys).map_pyerr(py)?;

        let results = PyList::new(py, &[]);
        for key in not_uploaded {
            match key {
                StoreKey::HgId(key) => {
                    let key_tuple = from_key_to_tuple(py, &key);
                    results.append(py, key_tuple.into_object());
                }
                StoreKey::Content(_, _) => {
                    return Err(anyhow!("Unsupported key: {:?}", key)).map_pyerr(py);
                }
            }
        }

        Ok(results)
    }


    def metadata(&self, name: &PyPath, node: &PyBytes) -> PyResult<PyDict> {
        let key = StoreKey::hgid(to_key(py, name, node)?);
        let store = self.store(py);
        let res = py.allow_threads(|| store.metadata(key)).map_pyerr(py)?;

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

    def getsharedmutable(&self) -> PyResult<Self> {
        let store = self.store(py);
        Self::create_instance(py, Arc::new(store.with_shared_only()))
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
    edenapi_treestore: Option<Arc<SaplingRemoteApiTreeStore>>,
    filestore: Option<Arc<FileStore>>,
    suffix: Option<String>,
) -> Result<Arc<TreeStore>> {
    let mut treestore_builder = TreeStoreBuilder::new(&config);

    if let Some(path) = path {
        treestore_builder = treestore_builder.local_path(path);
    }

    if let Some(ref suffix) = suffix {
        treestore_builder = treestore_builder.suffix(suffix);
    }

    // Extract SaplingRemoteApiAdapter for scmstore construction later on
    if let Some(edenapi) = edenapi_treestore {
        treestore_builder = treestore_builder.edenapi(edenapi.clone());
    }

    if let Some(filestore) = filestore {
        treestore_builder = treestore_builder.filestore(filestore);
    }

    let indexedlog_local = treestore_builder.build_indexedlog_local()?;
    let indexedlog_cache = treestore_builder.build_indexedlog_cache()?;

    if let Some(indexedlog_local) = indexedlog_local {
        treestore_builder = treestore_builder.indexedlog_local(indexedlog_local.clone());
    }

    if let Some(ref cache) = indexedlog_cache {
        treestore_builder = treestore_builder.indexedlog_cache(cache.clone());
    }

    Ok(Arc::new(treestore_builder.build()?))
}

py_class!(pub class treescmstore |py| {
    data store: Arc<TreeStore>;
    // Caching wrapper around store.
    data caching_store: Option<Arc<dyn storemodel::TreeStore>>;

    def __new__(_cls,
        path: Option<PyPathBuf>,
        config: config,
        edenapi: Option<edenapitreestore> = None,
        filestore: Option<filescmstore> = None,
        suffix: Option<String> = None,
    ) -> PyResult<Self> {
        // Extract Rust Values
        let path = path.as_ref().map(|v| v.as_path());
        let config = config.get_cfg(py);
        let edenapi = edenapi.map(|v| v.extract_inner(py));
        let filestore = filestore.map(|v| v.extract_inner(py));

        let treestore = make_treescmstore(path, &config, edenapi, filestore, suffix).map_pyerr(py)?;

        Self::create_instance(py, treestore, None)
    }

    def get(&self, name: PyPathBuf, node: &PyBytes) -> PyResult<PyBytes> {
        if let Some(caching_store) = &self.caching_store(py) {
            let repo_path = name.to_repo_path().map_pyerr(py)?;
            let node = to_node(py, node);
            return py.allow_threads(|| caching_store.get_content(
                FetchContext::default(),
                repo_path,
                node,
            )).map_pyerr(py)
              .map(|bytes| PyBytes::new(py, &bytes.into_bytes()[..]));
        }

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

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        let store = self.store(py);
        HgIdDataStorePyExt::get_missing_py(store, py, &mut keys.iter(py)?)
    }

    def add(&self, name: PyPathBuf, node: &PyBytes, deltabasenode: &PyBytes, delta: &PyBytes, metadata: Option<PyDict> = None) -> PyResult<PyObject> {
        let store = self.store(py);
        HgIdMutableDeltaStorePyExt::add_py(store, py, &name, node, deltabasenode, delta, metadata)
    }

    def flush(&self) -> PyResult<Option<Vec<PyPathBuf>>> {
        let store = self.store(py);
        HgIdMutableDeltaStorePyExt::flush_py(store, py)
    }

    def prefetch(&self, keys: PyList) -> PyResult<PyObject> {
        if let Some(caching_store) = &self.caching_store(py) {
            let keys =  keys
                .iter(py)
                .map(|tuple| from_tuple_to_key(py, &tuple))
                .collect::<PyResult<Vec<Key>>>()?;
            py.allow_threads(|| caching_store.prefetch(keys)).map_pyerr(py)?;
            return Ok(Python::None(py));
        }

        let store = self.store(py);

        let keys = keys
            .iter(py)
            .map(|tuple| from_tuple_to_key(py, &tuple))
            .collect::<PyResult<Vec<Key>>>()?;
        py.allow_threads(|| TreeStore::prefetch(store, keys)).map_pyerr(py)?;

        Ok(Python::None(py))
    }

    def markforrefresh(&self) -> PyResult<PyNone> {
        let store = self.store(py);
        HgIdDataStorePyExt::refresh_py(store, py)
    }

    def getsharedmutable(&self) -> PyResult<Self> {
        let store = self.store(py);
        Self::create_instance(py, Arc::new(TreeStore::with_shared_only(store)), None)
    }

    def metadatastore(&self) -> PyResult<metadatastore> {
        let store = self.store(py);
        metadatastore::create_instance(py, store.clone())
    }
});

impl ExtractInnerRef for treescmstore {
    type Inner = Arc<TreeStore>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.store(py)
    }
}
