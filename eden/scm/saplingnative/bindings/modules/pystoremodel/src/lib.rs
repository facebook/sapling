/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::convert::Serde;
use cpython_ext::PyIter;
use cpython_ext::ResultPyErrExt;
use storemodel::Bytes;
use storemodel::FileStore as NativeFileStore;
use storemodel::InsertOpts;
use storemodel::KeyStore as NativeKeyStore;
use storemodel::Kind;
use storemodel::SerializationFormat;
use storemodel::TreeEntry as NativeTreeEntry;
use storemodel::TreeItemFlag;
use storemodel::TreeStore as NativeTreeStore;
use types::fetch_mode::FetchMode;
use types::Id20;
use types::PathComponent;
use types::PathComponentBuf;
use types::RepoPath;
mod key;
use key::CompactKey;
use key::IntoCompactKey as _;
use key::IntoKeys as _;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "storemodel"].join(".");

    let m = PyModule::new(py, &name)?;
    m.add_class::<FileStore>(py)?;
    m.add_class::<KeyStore>(py)?;
    m.add_class::<TreeEntry>(py)?;
    m.add_class::<TreeStore>(py)?;
    m.add(
        py,
        "deserialize_tree",
        py_fn!(py, deserialize_tree(data: Serde<Bytes>, format: Serde<SerializationFormat>)),
    )?;
    m.add( py, "serialize_tree", py_fn!(py, serialize_tree(items: Serde<Vec<(PathComponentBuf, Id20, TreeItemFlag)>>, format: Serde<SerializationFormat>)))?;

    Ok(m)
}

py_class!(pub class KeyStore |py| {
    data inner: Arc<dyn NativeKeyStore>;

    /// get_content_iter(keys, fetch_mode = (AllowRemote)) -> Iterator[Tuple[Key, bytes]]
    def get_content_iter(&self, keys: Serde<Vec<CompactKey>>, fetch_mode: Serde<FetchMode> = Serde(FetchMode::AllowRemote)) -> PyResult<PyIter> {
        let inner = self.inner(py);
        let iter = inner.get_content_iter(keys.0.into_keys(), fetch_mode.0).map_pyerr(py)?;
        let iter = iter.into_compact_key();
        let iter = PyIter::new(py, iter)?;
        Ok(iter)
    }

    /// get_local_content(path, node) -> None | bytes
    def get_local_content(&self, path: &str, id: Serde<Id20>) -> PyResult<Serde<Option<Bytes>>> {
        let inner = self.inner(py);
        let path = RepoPath::from_str(path).map_pyerr(py)?;
        let result = py.allow_threads(|| inner.get_local_content(path, id.0)).map_pyerr(py)?;
        Ok(Serde(result))
    }

    /// get_content(path, node, fetch_mode = (AllowRemote)) -> None | bytes
    def get_content(&self, path: &str, id: Serde<Id20>, fetch_mode: Serde<FetchMode> = Serde(FetchMode::AllowRemote)) -> PyResult<Serde<Bytes>> {
        let inner = self.inner(py);
        let path = RepoPath::from_str(path).map_pyerr(py)?;
        let result = py.allow_threads(|| inner.get_content(path, id.0, fetch_mode.0)).map_pyerr(py)?;
        Ok(Serde(result))
    }

    /// prefetch(keys)
    def prefetch(&self, keys: Serde<Vec<CompactKey>>) -> PyResult<PyNone> {
        let inner = self.inner(py);
        py.allow_threads(|| inner.prefetch(keys.0.into_keys())).map_pyerr(py)?;
        Ok(PyNone)
    }

    def flush(&self) -> PyResult<PyNone> {
        let inner = self.inner(py);
        py.allow_threads(|| inner.flush()).map_pyerr(py)?;
        Ok(PyNone)
    }

    def refresh(&self) -> PyResult<PyNone> {
        let inner = self.inner(py);
        py.allow_threads(|| inner.refresh()).map_pyerr(py)?;
        Ok(PyNone)
    }

    def format(&self) -> PyResult<Serde<SerializationFormat>> {
        let inner = self.inner(py);
        Ok(Serde(inner.format()))
    }

    def statistics(&self) -> PyResult<Vec<(String, usize)>> {
        let inner = self.inner(py);
        Ok(inner.statistics())
    }

    def type_name(&self) -> PyResult<String> {
        let inner = self.inner(py);
        Ok(inner.type_name().into_owned())
    }

    @staticmethod
    def from_store(store: ImplInto<Arc<dyn NativeKeyStore>>) -> PyResult<Self> {
        let inner = store.into();
        Self::create_instance(py, inner)
    }
});

py_class!(pub class FileStore |py| {
    data inner: Arc<dyn NativeFileStore>;

    /// get_rename_iter(keys) -> Iteratable[Tuple[Key, Key]]
    def get_rename_iter(&self, keys: Serde<Vec<CompactKey>>) -> PyResult<PyIter> {
        let inner = self.inner(py);
        let iter = inner.get_rename_iter(keys.0.into_keys()).map_pyerr(py)?;
        let iter = iter.map(|v| v.map(|(k1, k2)| (CompactKey::from_key(k1), CompactKey::from_key(k2))));
        let iter = PyIter::new(py, iter)?;
        Ok(iter)
    }

    /// get_hg_parents(path, node) -> List[node]
    /// This is only used by legacy Hg logic and is incompatible with Git.
    def get_hg_parents(&self, path: &str, id: Serde<Id20>) -> PyResult<Serde<Vec<Id20>>> {
        let inner = self.inner(py);
        let path = RepoPath::from_str(path).map_pyerr(py)?;
        let result = py.allow_threads(|| inner.get_hg_parents(path, id.0)).map_pyerr(py)?;
        Ok(Serde(result))
    }

    /// get_hg_raw_content(path, node) -> bytes
    /// This is only used by legacy Hg logic and is incompatible with Git.
    def get_hg_raw_content(&self, path: &str, id: Serde<Id20>) -> PyResult<Serde<Bytes>> {
        let inner = self.inner(py);
        let path = RepoPath::from_str(path).map_pyerr(py)?;
        let result = py.allow_threads(|| inner.get_hg_raw_content(path, id.0)).map_pyerr(py)?;
        Ok(Serde(result))
    }

    /// get_hg_flags(path, node) -> int
    /// This is only used by legacy Hg logic and is incompatible with Git.
    def get_hg_flags(&self, path: &str, id: Serde<Id20>) -> PyResult<u32> {
        let inner = self.inner(py);
        let path = RepoPath::from_str(path).map_pyerr(py)?;
        let result = py.allow_threads(|| inner.get_hg_flags(path, id.0)).map_pyerr(py)?;
        Ok(result)
    }

    /// Upload LFS files specified by the keys.
    /// This is called before push.
    def upload_lfs(&self, keys: Serde<Vec<CompactKey>>) -> PyResult<PyNone> {
        let inner = self.inner(py);
        py.allow_threads(|| inner.upload_lfs(keys.0.into_keys())).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// insert_file(opts, path: str, data: bytes) -> node
    /// opts: {parents: List[node], hg_flags: int}
    ///
    /// Check `storemodel::FileStore` for details.
    def insert_file(&self, opts: Serde<InsertOpts>, path: &str, data: PyBytes) -> PyResult<Serde<Id20>> {
        let inner = self.inner(py);
        let path = RepoPath::from_str(path).map_pyerr(py)?;
        let data = data.data(py);
        let id = py.allow_threads(|| inner.insert_file(opts.0, path, data)).map_pyerr(py)?;
        Ok(Serde(id))
    }

    def flush(&self) -> PyResult<PyNone> {
        let inner = self.inner(py);
        py.allow_threads(|| inner.flush()).map_pyerr(py)?;
        Ok(PyNone)
    }

    def as_key_store(&self) -> PyResult<KeyStore> {
        let inner = self.inner(py);
        let store = inner.clone_key_store();
        KeyStore::create_instance(py, store.into())
    }

    def type_name(&self) -> PyResult<String> {
        let inner = self.inner(py);
        Ok(inner.type_name().into_owned())
    }

    @staticmethod
    def from_store(store: ImplInto<Arc<dyn NativeFileStore>>) -> PyResult<Self> {
        let inner = store.into();
        Self::create_instance(py, inner)
    }
});

py_class!(pub class TreeEntry |py| {
    data inner: Box<dyn NativeTreeEntry>;

    def __iter__(&self) -> PyResult<PyIter> {
        let inner = self.inner(py);
        let iter = inner.iter().map_pyerr(py)?;
        let iter = PyIter::new(py, iter)?;
        Ok(iter)
    }

    /// lookup(name) -> (node, flag) | None
    def lookup(&self, name: &str) -> PyResult<Serde<Option<(Id20, TreeItemFlag)>>> {
        let inner = self.inner(py);
        let name = PathComponent::from_str(name).map_pyerr(py)?;
        let result = inner.lookup(name).map_pyerr(py)?;
        Ok(Serde(result))
    }
});

py_class!(pub class TreeStore |py| {
    data inner: Arc<dyn NativeTreeStore>;

    /// get_local_tree(path, node) -> TreeEntry
    def get_local_tree(&self, path: &str, id: Serde<Id20>) -> PyResult<Option<TreeEntry>> {
        let inner = self.inner(py);
        let path = RepoPath::from_str(path).map_pyerr(py)?;
        match py.allow_threads(|| inner.get_local_tree(path, id.0)).map_pyerr(py)? {
            Some(entry) => Ok(Some(TreeEntry::create_instance(py, entry)?)),
            None => Ok(None),
        }
    }

    /// get_remote_tree_iter(keys) -> Iterator[Tuple[Key, TreeEntry]]
    def get_remote_tree_iter(&self, keys: Serde<Vec<CompactKey>>) -> PyResult<PyIter> {
        let inner = self.inner(py);
        let iter = inner.get_remote_tree_iter(keys.0.into_keys()).map_pyerr(py)?;
        PyIter::new_custom(py, iter, |py, (key, entry)| {
            Ok((Serde(key.into_compact_key()), TreeEntry::create_instance(py, entry)?).to_py_object(py).into_object())
        })
    }

    /// get_tree_iter(keys, fetch_mode) -> Iterator[Tuple[Key, TreeEntry]]
    def get_tree_iter(&self, keys: Serde<Vec<CompactKey>>, fetch_mode: Serde<FetchMode> = Serde(FetchMode::AllowRemote)) -> PyResult<PyIter> {
        let inner = self.inner(py);
        let iter = inner.get_tree_iter(keys.0.into_keys(), fetch_mode.0).map_pyerr(py)?;
        PyIter::new_custom(py, iter, |py, (key, entry)| {
            Ok((Serde(key.into_compact_key()), TreeEntry::create_instance(py, entry)?).to_py_object(py).into_object())
        })
    }

    /// insert_tree(opts, path: str, items: [(name, node, flag)]) -> node
    /// flag: 'directory' | {'file': 'regular' | 'executable' | 'symlink' | 'git_submodule'})
    /// opts: {parents: List[node], hg_flags: int}
    ///
    /// Check `storemodel::TreeStore` for details.
    def insert_tree(&self, opts: Serde<InsertOpts>, path: &str, items: Serde<Vec<(PathComponentBuf, Id20, TreeItemFlag)>>) -> PyResult<Serde<Id20>> {
        let inner = self.inner(py);
        let path = RepoPath::from_str(path).map_pyerr(py)?;
        let id = py.allow_threads(|| inner.insert_tree(opts.0, path, items.0)).map_pyerr(py)?;
        Ok(Serde(id))
    }

    /// insert_data(opts, path, data: bytes) -> node
    /// opts: {parents: List[node], hg_flags: int}
    ///
    /// The callsite takes care of serialization.
    /// `data` does not include Git or Hg SHA1 headers.
    ///
    /// Check `storemodel::KeyStore` for details.
    def insert_data(&self, opts: Serde<InsertOpts>, path: &str, data: PyBytes) -> PyResult<Serde<Id20>> {
        let mut opts = opts.0;
        opts.kind = Kind::Tree;
        let path = RepoPath::from_str(path).map_pyerr(py)?;
        let data = data.data(py);
        let inner = self.inner(py);
        let id = py.allow_threads(|| inner.insert_data(opts, path, data)).map_pyerr(py)?;
        Ok(Serde(id))
    }

    def flush(&self) -> PyResult<PyNone> {
        let inner = self.inner(py);
        py.allow_threads(|| inner.flush()).map_pyerr(py)?;
        Ok(PyNone)
    }

    def format(&self) -> PyResult<Serde<SerializationFormat>> {
        let inner = self.inner(py);
        Ok(Serde(inner.format()))
    }

    def as_key_store(&self) -> PyResult<KeyStore> {
        let inner = self.inner(py);
        let store = inner.clone_key_store();
        KeyStore::create_instance(py, store.into())
    }

    def type_name(&self) -> PyResult<String> {
        let inner = self.inner(py);
        Ok(inner.type_name().into_owned())
    }

    @staticmethod
    def from_store(store: ImplInto<Arc<dyn NativeTreeStore>>) -> PyResult<Self> {
        let inner = store.into();
        Self::create_instance(py, inner)
    }
});

fn deserialize_tree(
    py: Python,
    data: Serde<Bytes>,
    format: Serde<SerializationFormat>,
) -> PyResult<Serde<Vec<(PathComponentBuf, Id20, TreeItemFlag)>>> {
    let tree_entry = storemodel::basic_parse_tree(data.0, format.0).map_pyerr(py)?;
    let iter = tree_entry.iter().map_pyerr(py)?;
    let result = iter.collect::<Result<Vec<_>, _>>().map_pyerr(py)?;
    Ok(Serde(result))
}

fn serialize_tree(
    py: Python,
    items: Serde<Vec<(PathComponentBuf, Id20, TreeItemFlag)>>,
    format: Serde<SerializationFormat>,
) -> PyResult<Serde<Bytes>> {
    let bytes = storemodel::basic_serialize_tree(items.0, format.0).map_pyerr(py)?;
    Ok(Serde(bytes))
}
