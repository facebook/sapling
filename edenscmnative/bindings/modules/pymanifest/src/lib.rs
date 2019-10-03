// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![allow(non_camel_case_types)]

use std::{borrow::Borrow, cell::RefCell, ops::Deref, str, sync::Arc};

use bytes::Bytes;
use cpython::*;
use failure::{format_err, Fallible};

use cpython_ext::{pyset_add, pyset_new};
use cpython_failure::ResultPyErrExt;
use encoding::{local_bytes_to_repo_path, repo_path_to_local_bytes};
use manifest::{self, Diff, DiffType, FileMetadata, FileType, FsNode, Manifest};
use pathmatcher::{AlwaysMatcher, Matcher};
use pypathmatcher::PythonMatcher;
use pyrevisionstore::PythonDataStore;
use revisionstore::{DataStore, RemoteDataStore};
use types::{Key, Node, RepoPath, RepoPathBuf};

struct ManifestStore<T> {
    underlying: T,
}

impl<T> ManifestStore<T> {
    pub fn new(underlying: T) -> Self {
        ManifestStore { underlying }
    }
}

impl<T: DataStore + RemoteDataStore> manifest::TreeStore for ManifestStore<T> {
    fn get(&self, path: &RepoPath, node: Node) -> Fallible<Bytes> {
        let key = Key::new(path.to_owned(), node);
        self.underlying
            .get(&key)?
            .ok_or_else(|| format_err!("Key {:?} not found in manifest", key))
            .map(|data| Bytes::from(data))
    }

    fn insert(&self, _path: &RepoPath, _node: Node, _data: Bytes) -> Fallible<()> {
        unimplemented!(
            "At this time we don't expect to ever write manifest in rust using python stores."
        );
    }

    fn prefetch(&self, keys: Vec<Key>) -> Fallible<()> {
        self.underlying.prefetch(keys)
    }
}

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "manifest"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<treemanifest>(py)?;
    m.add(
        py,
        "subdirdiff",
        py_fn!(
            py,
            subdir_diff(
                store: PyObject,
                path: &PyBytes,
                binnode: &PyBytes,
                other_binnodes: &PyList,
                depth: i32
            )
        ),
    )?;
    m.add(
        py,
        "prefetch",
        py_fn!(
            py,
            prefetch(
                store: PyObject,
                node: &PyBytes,
                path: &PyBytes,
                depth: Option<usize> = None
            )
        ),
    )?;
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
    // When the `path` does not exist, it return a KeyError.
    def find(&self, path: &PyBytes) -> PyResult<(PyBytes, String)> {
        let repo_path = pybytes_to_path(py, path);
        let tree = self.underlying(py).borrow();
        match tree.get_file(&repo_path).map_pyerr::<exc::RuntimeError>(py)? {
            None => {
                let msg = format!("cannot find file '{}' in manifest", repo_path);
                Err(PyErr::new::<exc::KeyError, _>(py, msg))
            }
            Some(file_metadata) => file_metadata_to_py_tuple(py, &file_metadata),
        }
    }

    def get(&self, path: &PyBytes, default: Option<PyBytes> = None) -> PyResult<Option<PyBytes>> {
        let repo_path = pybytes_to_path(py, path);
        let tree = self.underlying(py).borrow();
        let result = match tree.get_file(&repo_path).map_pyerr::<exc::RuntimeError>(py)? {
            None => None,
            Some(file_metadata) => Some(node_to_pybytes(py, file_metadata.node)),
        };
        Ok(result.or(default))
    }

    def flags(&self, path: &PyBytes, default: Option<PyString> = None) -> PyResult<PyString> {
        let repo_path = pybytes_to_path(py, path);
        let tree = self.underlying(py).borrow();
        let result = match tree.get_file(&repo_path).map_pyerr::<exc::RuntimeError>(py)? {
            None => None,
            Some(file_metadata) => Some(file_type_to_pystring(py, file_metadata.file_type)),
        };
        Ok(result.or(default).unwrap_or_else(|| PyString::new(py, "")))
    }

    def hasdir(&self, path: &PyBytes) -> PyResult<bool> {
        let repo_path = pybytes_to_path(py, path);
        let tree = self.underlying(py).borrow();
        let result = match tree.get(&repo_path).map_pyerr::<exc::RuntimeError>(py)? {
            Some(FsNode::Directory) => true,
            _ => false
        };
        Ok(result)
    }

    // Returns a list<path> for all files that match the predicate passed to the function.
    def walk(&self, pymatcher: PyObject) -> PyResult<Vec<PyBytes>> {
        let mut result = Vec::new();
        let tree = self.underlying(py).borrow();
        for entry in tree.files(&PythonMatcher::new(py, pymatcher)) {
            let file = entry.map_pyerr::<exc::RuntimeError>(py)?;
            result.push(path_to_pybytes(py, &file.path));
        }
        Ok(result)
    }

    def listdir(&self, path: &PyBytes) -> PyResult<Vec<PyBytes>> {
        let repo_path = pybytes_to_path(py, path);
        let tree = self.underlying(py).borrow();
        let result = match tree.list(&repo_path).map_pyerr::<exc::RuntimeError>(py)? {
            manifest::tree::List::NotFound | manifest::tree::List::File => vec![],
            manifest::tree::List::Directory(components) =>
                components.into_iter().map(|component|
                    path_to_pybytes(py, component.as_path_component())
                ).collect()
        };
        Ok(result)
    }

    def text(&self) -> PyResult<PyBytes> {
        let mut lines = Vec::new();
        let tree = self.underlying(py).borrow();
        for entry in tree.files(&AlwaysMatcher::new()) {
            let file = entry.map_pyerr::<exc::RuntimeError>(py)?;
            lines.push(format!(
                "{}\0{}{}\n",
                file.path,
                file.meta.node,
                file_type_to_str(file.meta.file_type)
            ));
        }
        lines.sort();
        // TODO: Optimize this so that the string does not get copied.
        Ok(PyBytes::new(py, lines.concat().as_bytes()))
    }

    def set(&self, path: &PyBytes, binnode: &PyBytes, flag: &PyString) -> PyResult<PyObject> {
        // TODO: can the node and flag that are passed in be None?
        let mut tree = self.underlying(py).borrow_mut();
        let repo_path = pybytes_to_path(py, path);
        let node = pybytes_to_node(py, binnode)?;
        let file_type = pystring_to_file_type(py, flag)?;
        let file_metadata = FileMetadata::new(node, file_type);
        tree.insert(repo_path, file_metadata).map_pyerr::<exc::RuntimeError>(py)?;
        Ok(py.None())
    }

    def setflag(&self, path: &PyBytes, flag: &PyString) -> PyResult<PyObject> {
        let mut tree = self.underlying(py).borrow_mut();
        let repo_path = pybytes_to_path(py, path);
        let file_type = pystring_to_file_type(py, flag)?;
        let file_metadata = match tree.get_file(&repo_path).map_pyerr::<exc::RuntimeError>(py)? {
            None => {
                let msg = "cannot setflag on file that is not in manifest";
                return Err(PyErr::new::<exc::KeyError, _>(py, msg));
            }
            Some(mut file_metadata) => {
                file_metadata.file_type = file_type;
                file_metadata
            }
        };
        tree.insert(repo_path, file_metadata).map_pyerr::<exc::RuntimeError>(py)?;
        Ok(Python::None(py))
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

        for entry in Diff::new(&this_tree, &other_tree, &matcher) {
            let entry = entry.map_pyerr::<exc::RuntimeError>(py)?;
            let path = path_to_pybytes(py, &entry.path);
            let diff_left = convert_side_diff(py, entry.diff_type.left());
            let diff_right = convert_side_diff(py, entry.diff_type.right());
            result.set_item(py, path, (diff_left, diff_right))?;
        }
        Ok(result)
    }


    def filesnotin(
        &self,
        other: &treemanifest,
        matcher: Option<PyObject> = None
    ) -> PyResult<PyObject> {
        let mut result = pyset_new(py)?;
        let this_tree = self.underlying(py).borrow();
        let other_tree = other.underlying(py).borrow();
        let matcher: Box<dyn Matcher> = match matcher {
            None => Box::new(AlwaysMatcher::new()),
            Some(pyobj) => Box::new(PythonMatcher::new(py, pyobj)),
        };
        for entry in Diff::new(&this_tree, &other_tree, &matcher) {
            let entry = entry.map_pyerr::<exc::RuntimeError>(py)?;
            match entry.diff_type {
                DiffType::LeftOnly(_) => {
                    pyset_add(py, &mut result, path_to_pybytes(py, &entry.path))?;
                }
                DiffType::RightOnly(_) | DiffType::Changed(_, _) => (),
            }
        }
        Ok(result)
    }

    def matches(&self, pymatcher: PyObject) -> PyResult<PyObject> {
        let manifestmod = py.import("edenscm.mercurial.manifest")?;
        let manifestdict = manifestmod.get(py, "manifestdict")?;
        let result = manifestdict.call(py, NoArgs, None)?;
        let tree = self.underlying(py).borrow();
        for entry in tree.files(&PythonMatcher::new(py, pymatcher)) {
            let file = entry.map_pyerr::<exc::RuntimeError>(py)?;
            let pypath = path_to_pybytes(py, &file.path);
            let pynode = node_to_pybytes(py, file.meta.node);
            result.call_method(py, "__setitem__", (pypath, pynode), None)?;
            let pypath = path_to_pybytes(py, &file.path);
            let pyflags = file_type_to_pystring(py, file.meta.file_type);
            result.call_method(py, "setflag", (pypath, pyflags), None)?;
        }
        Ok(result)
    }

    def __setitem__(&self, path: &PyBytes, binnode: &PyBytes) -> PyResult<()> {
        let mut tree = self.underlying(py).borrow_mut();
        let repo_path = pybytes_to_path(py, path);
        let node = pybytes_to_node(py, binnode)?;
        let file_metadata = match tree.get_file(&repo_path).map_pyerr::<exc::RuntimeError>(py)? {
            None => FileMetadata::new(node, FileType::Regular),
            Some(mut file_metadata) => {
                file_metadata.node = node;
                file_metadata
            }
        };
        tree.insert(repo_path, file_metadata).map_pyerr::<exc::RuntimeError>(py)?;
        Ok(())
    }

    def __delitem__(&self, path: &PyBytes) -> PyResult<()> {
        let mut tree = self.underlying(py).borrow_mut();
        let repo_path = pybytes_to_path(py, path);
        tree.remove(&repo_path).map_pyerr::<exc::RuntimeError>(py)?;
        Ok(())
    }

    def __getitem__(&self, key: &PyBytes) -> PyResult<PyBytes> {
        let path = pybytes_to_path(py, key);
        let tree = self.underlying(py).borrow();
        match tree.get_file(&path).map_pyerr::<exc::RuntimeError>(py)? {
            Some(file_metadata) => Ok(node_to_pybytes(py, file_metadata.node)),
            None => Err(PyErr::new::<exc::KeyError, _>(py, format!("file {} not found", path))),
        }
    }

    def keys(&self) -> PyResult<Vec<PyBytes>> {
        let mut result = Vec::new();
        let tree = self.underlying(py).borrow();
        for entry in tree.files(&AlwaysMatcher::new()) {
            let file = entry.map_pyerr::<exc::RuntimeError>(py)?;
            result.push(path_to_pybytes(py, &file.path));
        }
        Ok(result)
    }

    // iterator stuff

    def __contains__(&self, key: &PyBytes) -> PyResult<bool> {
        let path = pybytes_to_path(py, key);
        let tree = self.underlying(py).borrow();
        match tree.get_file(&path).map_pyerr::<exc::RuntimeError>(py)? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    def __iter__(&self) -> PyResult<PyObject> {
        let mut result = Vec::new();
        let tree = self.underlying(py).borrow();
        for entry in tree.files(&AlwaysMatcher::new()) {
            let file = entry.map_pyerr::<exc::RuntimeError>(py)?;
            result.push(path_to_pybytes(py, &file.path));
        }
        vec_to_iter(py, result)
    }

    def iteritems(&self) -> PyResult<PyObject> {
        let mut result = Vec::new();
        let tree = self.underlying(py).borrow();
        for entry in tree.files(&AlwaysMatcher::new()) {
            let file = entry.map_pyerr::<exc::RuntimeError>(py)?;
            let tuple = (
                path_to_pybytes(py, &file.path),
                node_to_pybytes(py, file.meta.node),
            );
            result.push(tuple);
        }
        vec_to_iter(py, result)
    }

    def iterkeys(&self) -> PyResult<PyObject> {
        let mut result = Vec::new();
        let tree = self.underlying(py).borrow();
        for entry in tree.files(&AlwaysMatcher::new()) {
            let file = entry.map_pyerr::<exc::RuntimeError>(py)?;
            result.push(path_to_pybytes(py, &file.path));
        }
        vec_to_iter(py, result)
    }

    def finalize(
        &self,
        p1tree: Option<&treemanifest> = None,
        p2tree: Option<&treemanifest> = None
    ) -> PyResult<Vec<PyTuple>> {
        let mut result = Vec::new();
        let mut tree = self.underlying(py).borrow_mut();
        let mut parents = vec!();
        if let Some(m1) = p1tree {
            parents.push(m1.underlying(py).borrow());
        }
        if let Some(m2) = p2tree {
            parents.push(m2.underlying(py).borrow());
        }
        let entries = tree.finalize(
            parents.iter().map(|x| x.deref()).collect()
        ).map_pyerr::<exc::RuntimeError>(py)?;
        for entry in entries {
            let (repo_path, node, raw, p1node, p2node) = entry;
            let tuple = PyTuple::new(
                py,
                &[
                    path_to_pybytes(py, &repo_path).into_object(),
                    node_to_pybytes(py, node).into_object(),
                    PyBytes::new(py, &raw).into_object(),
                    PyBytes::new(py, &[]).into_object(),
                    node_to_pybytes(py, p1node).into_object(),
                    node_to_pybytes(py, p2node).into_object(),
                ],
            );
            result.push(tuple);
        }
        Ok(result)
    }
});

pub fn subdir_diff(
    py: Python,
    store: PyObject,
    path: &PyBytes,
    binnode: &PyBytes,
    other_binnodes: &PyList,
    depth: i32,
) -> PyResult<PyObject> {
    let store = PythonDataStore::new(store);
    let manifest_store = Arc::new(ManifestStore::new(store));
    let mut others = vec![];
    for pybytes in other_binnodes.iter(py) {
        others.push(pybytes_to_node(py, &pybytes.extract(py)?)?);
    }
    let diff = manifest::compat_subtree_diff(
        manifest_store,
        &pybytes_to_path(py, path),
        pybytes_to_node(py, binnode)?,
        others,
        depth,
    )
    .map_pyerr::<exc::RuntimeError>(py)?;
    let mut result = vec![];
    for (path, node, bytes) in diff {
        let tuple = PyTuple::new(
            py,
            &[
                path_to_pybytes(py, &path).into_object(),
                node_to_pybytes(py, node).into_object(),
                PyBytes::new(py, &bytes).into_object(),
                py.None(),
                py.None(),
                py.None(),
            ],
        );
        result.push(tuple);
    }
    vec_to_iter(py, result)
}

pub fn prefetch(
    py: Python,
    store: PyObject,
    node: &PyBytes,
    path: &PyBytes,
    depth: Option<usize>,
) -> PyResult<PyObject> {
    let store = Arc::new(ManifestStore::new(PythonDataStore::new(store)));
    let node = pybytes_to_node(py, node)?;
    let path = pybytes_to_path(py, path);
    let key = Key::new(path, node);
    manifest::prefetch(store, key, depth).map_pyerr::<exc::RuntimeError>(py)?;
    Ok(py.None())
}

fn vec_to_iter<T: ToPyObject>(py: Python, items: Vec<T>) -> PyResult<PyObject> {
    let list: PyList = items.into_py_object(py);
    list.into_object().call_method(py, "__iter__", NoArgs, None)
}

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

fn path_to_pybytes<T: AsRef<RepoPath>>(py: Python, path: T) -> PyBytes {
    PyBytes::new(py, repo_path_to_local_bytes(path.as_ref()))
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
    PyString::new(py, file_type_to_str(file_type))
}

fn file_type_to_str(file_type: FileType) -> &'static str {
    match file_type {
        FileType::Regular => "",
        FileType::Executable => "x",
        FileType::Symlink => "l",
    }
}
