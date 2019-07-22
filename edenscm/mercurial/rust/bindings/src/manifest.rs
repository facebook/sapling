// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{str, sync::Arc};

use bytes::Bytes;
use cpython::*;
use failure::Fallible;

use cpython_failure::ResultPyErrExt;
use encoding::{local_bytes_to_repo_path, repo_path_to_local_bytes};
use manifest::Manifest;
use revisionstore::DataStore;
use types::{Key, Node, RepoPath, RepoPathBuf};

use crate::revisionstore::PythonDataStore;

struct ManifestStore<T> {
    underlying: T,
}

impl<T> ManifestStore<T> {
    pub fn new(underlying: T) -> Self {
        ManifestStore { underlying }
    }
}

impl<T: DataStore> manifest::TreeStore for ManifestStore<T> {
    fn get(&self, path: &RepoPath, node: Node) -> Fallible<Bytes> {
        let key = Key::new(path.to_owned(), node);
        self.underlying.get(&key).map(|data| Bytes::from(data))
    }

    fn insert(&self, _path: &RepoPath, _node: Node, _data: Bytes) -> Fallible<()> {
        unimplemented!(
            "At this time we don't expect to ever write manifest in rust using python stores."
        );
    }
}

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "manifest"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<treemanifest>(py)?;
    Ok(m)
}

py_class!(class treemanifest |py| {
    data underlying: manifest::Tree;

    def __new__(
        _cls,
        store: PyObject,
        node: Option<&PyBytes> = None
    ) -> PyResult<treemanifest> {
        let store = PythonDataStore::new(store);
        let manifest_store = Arc::new(ManifestStore::new(store));
        let underlying = match node {
            None => manifest::Tree::ephemeral(manifest_store),
            Some(value) => manifest::Tree::durable(manifest_store, pybytes_to_node(py, value)?),
        };
        treemanifest::create_instance(py, underlying)
    }

    // Returns (node, flag) for a given `path` in the manifest.
    def find(&self, path: &PyBytes) -> PyResult<Option<(PyBytes, String)>> {
        let repo_path = pybytes_to_path(py, path);
        let tree = &self.underlying(py);
        let result = match tree.get(&repo_path).map_pyerr::<exc::RuntimeError>(py)? {
            None => None,
            Some(file_metadata) => Some(file_metadata_to_py_tuple(py, file_metadata)?),
        };
        Ok(result)
    }

    // Returns a list<path> for all files that match the predicate passed to the function.
    def walk(&self, _matcher: Option<PyObject> = None) -> PyResult<Vec<PyBytes>> {
        let mut result = Vec::new();
        let manifest = self.underlying(py);
        let files = manifest.files();
        for entry in files {
            let (path, _) = entry.map_pyerr::<exc::RuntimeError>(py)?;
            result.push(path_to_pybytes(py, &path));
        }
        Ok(result)
    }
});

fn file_metadata_to_py_tuple(
    py: Python,
    file_metadata: &manifest::FileMetadata,
) -> PyResult<(PyBytes, String)> {
    let node = PyBytes::new(py, file_metadata.node.as_ref());
    let flag = {
        let mut s = String::new();
        match file_metadata.file_type {
            manifest::FileType::Regular => (),
            manifest::FileType::Executable => s.push('x'),
            manifest::FileType::Symlink => s.push('l'),
        };
        s
    };
    Ok((node, flag))
}

fn pybytes_to_node(py: Python, pybytes: &PyBytes) -> PyResult<Node> {
    Node::from_slice(pybytes.data(py)).map_pyerr::<exc::ValueError>(py)
}

fn pybytes_to_path(py: Python, pybytes: &PyBytes) -> RepoPathBuf {
    local_bytes_to_repo_path(pybytes.data(py)).to_owned()
}

fn path_to_pybytes(py: Python, path: &RepoPath) -> PyBytes {
    PyBytes::new(py, repo_path_to_local_bytes(path))
}
