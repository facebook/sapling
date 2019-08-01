// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{borrow::Borrow, cell::RefCell, str, sync::Arc};

use bytes::Bytes;
use cpython::*;
use failure::Fallible;

use cpython_failure::ResultPyErrExt;
use encoding::{local_bytes_to_repo_path, repo_path_to_local_bytes};
use manifest::{self, FileMetadata, FileType, Manifest};
use pathmatcher::{AlwaysMatcher, Matcher};
use revisionstore::DataStore;
use types::{Key, Node, RepoPath, RepoPathBuf};

use crate::pathmatcher::PythonMatcher;
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
    data underlying: RefCell<manifest::Tree>;

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
        treemanifest::create_instance(py, RefCell::new(underlying))
    }

    // Returns a new instance of treemanifest that contains the same data as the base.
    def copy(&self) -> PyResult<treemanifest> {
        let tree = self.underlying(py);
        treemanifest::create_instance(py, tree.clone())
    }

    // Returns (node, flag) for a given `path` in the manifest.
    def find(&self, path: &PyBytes) -> PyResult<Option<(PyBytes, String)>> {
        let repo_path = pybytes_to_path(py, path);
        let tree = self.underlying(py).borrow();
        let result = match tree.get(&repo_path).map_pyerr::<exc::RuntimeError>(py)? {
            None => None,
            Some(file_metadata) => Some(file_metadata_to_py_tuple(py, file_metadata)?),
        };
        Ok(result)
    }

    def flags(&self, path: &PyBytes, default: Option<PyString> = None) -> PyResult<PyString> {
        let repo_path = pybytes_to_path(py, path);
        let tree = self.underlying(py).borrow();
        let result = match tree.get(&repo_path).map_pyerr::<exc::RuntimeError>(py)? {
            None => None,
            Some(file_metadata) => Some(file_type_to_pystring(py, file_metadata.file_type)),
        };
        Ok(result.or(default).unwrap_or_else(|| PyString::new(py, "")))
    }

    // Returns a list<path> for all files that match the predicate passed to the function.
    def walk(&self, pymatcher: PyObject) -> PyResult<Vec<PyBytes>> {
        let mut result = Vec::new();
        let tree = self.underlying(py).borrow();
        for entry in tree.files(&PythonMatcher::new(py, pymatcher)) {
            let (path, _) = entry.map_pyerr::<exc::RuntimeError>(py)?;
            result.push(path_to_pybytes(py, &path));
        }
        Ok(result)
    }

    def set(&self, path: &PyBytes, binnode: &PyBytes, flag: &PyString) -> PyResult<PyObject> {
        let mut tree = self.underlying(py).borrow_mut();
        let repo_path = pybytes_to_path(py, path);
        let node = pybytes_to_node(py, binnode)?;
        let file_type = pystring_to_file_type(py, flag)?;
        let file_metadata = FileMetadata::new(node, file_type);
        tree.insert(repo_path, file_metadata).map_pyerr::<exc::RuntimeError>(py)?;
        Ok(py.None())
    }

    def diff(&self, other: &treemanifest, matcher: Option<PyObject> = None) -> PyResult<PyDict> {
        fn convert_side_diff(
            py: Python,
            entry: Option<FileMetadata>
        ) -> (Option<PyBytes>, PyString) {
            match entry {
                None => (None, PyString::new(py, "")),
                Some(file_metadata) => (
                    Some(node_to_pybytes(py, file_metadata.node)),
                    file_type_to_pystring(py, file_metadata.file_type)
                )
            }
        }

        let result = PyDict::new(py);
        let this_tree = self.underlying(py).borrow();
        let other_tree = other.underlying(py).borrow();
        let matcher: Box<dyn Matcher> = match matcher {
            None => Box::new(AlwaysMatcher::new()),
            Some(pyobj) => Box::new(PythonMatcher::new(py, pyobj)),
        };
        for entry in manifest::diff(&this_tree, &other_tree, &matcher) {
            let entry = entry.map_pyerr::<exc::RuntimeError>(py)?;
            let path = path_to_pybytes(py, &entry.path);
            let diff_left = convert_side_diff(py, entry.diff_type.left());
            let diff_right = convert_side_diff(py, entry.diff_type.right());
            result.set_item(py, path, (diff_left, diff_right))?;
        }
        Ok(result)
    }

    // iterator stuff

    def __contains__(&self, key: &PyBytes) -> PyResult<bool> {
        let path = pybytes_to_path(py, key);
        let tree = self.underlying(py).borrow();
        match tree.get(&path).map_pyerr::<exc::RuntimeError>(py)? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    def __getitem__(&self, key: &PyBytes) -> PyResult<PyBytes> {
        let path = pybytes_to_path(py, key);
        let tree = self.underlying(py).borrow();
        match tree.get(&path).map_pyerr::<exc::RuntimeError>(py)? {
            Some(file_metadata) => Ok(node_to_pybytes(py, file_metadata.node)),
            None => Err(PyErr::new::<exc::KeyError, _>(py, format!("file {} not found", path))),
        }
    }

    def iteritems(&self) -> PyResult<Vec<(PyBytes, PyBytes, PyString)>> {
        let mut result = Vec::new();
        let tree = self.underlying(py).borrow();
        for entry in tree.files(&AlwaysMatcher::new()) {
            let (path, file_metadata) = entry.map_pyerr::<exc::RuntimeError>(py)?;
            let tuple = (
                path_to_pybytes(py, &path),
                node_to_pybytes(py, file_metadata.node),
                file_type_to_pystring(py, file_metadata.file_type),
            );
            result.push(tuple);
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

fn node_to_pybytes(py: Python, node: Node) -> PyBytes {
    PyBytes::new(py, node.as_ref())
}

fn pybytes_to_path(py: Python, pybytes: &PyBytes) -> RepoPathBuf {
    local_bytes_to_repo_path(pybytes.data(py)).to_owned()
}

fn path_to_pybytes(py: Python, path: &RepoPath) -> PyBytes {
    PyBytes::new(py, repo_path_to_local_bytes(path))
}

fn pystring_to_file_type(py: Python, pystring: &PyString) -> PyResult<FileType> {
    match pystring.to_string_lossy(py).borrow() {
        "x" => Ok(FileType::Executable),
        "l" => Ok(FileType::Symlink),
        "" => Ok(FileType::Regular),
        _ => Err(PyErr::new::<exc::RuntimeError, _>(py, "invalid file flags")),
    }
}

fn file_type_to_pystring(py: Python, file_type: FileType) -> PyString {
    match file_type {
        FileType::Regular => PyString::new(py, ""),
        FileType::Executable => PyString::new(py, "x"),
        FileType::Symlink => PyString::new(py, "l"),
    }
}
