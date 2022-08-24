/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::borrow::Borrow;
use std::cell::RefCell;
use std::collections::HashSet;
use std::ops::Deref;
use std::str;
use std::sync::Arc;

use anyhow::Result;
use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::pyset_add;
use cpython_ext::pyset_new;
use cpython_ext::PyNone;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use manifest::DiffType;
use manifest::File;
use manifest::FileMetadata;
use manifest::FileType;
use manifest::FsNodeMetadata;
use manifest::Manifest;
use manifest_tree::TreeManifest;
use manifest_tree::TreeStore;
use parking_lot::RwLock;
use pathmatcher::AlwaysMatcher;
use pathmatcher::Matcher;
use pathmatcher::TreeMatcher;
use pypathmatcher::extract_matcher;
use pypathmatcher::extract_option_matcher;
use types::Key;
use types::Node;
use types::RepoPathBuf;

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
                store: ImplInto<Arc<dyn TreeStore + Send + Sync>>,
                path: PyPathBuf,
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
                store: ImplInto<Arc<dyn TreeStore + Send + Sync>>,
                node: &PyBytes,
                path: PyPathBuf,
                depth: Option<usize> = None
            )
        ),
    )?;
    Ok(m)
}

py_class!(pub class treemanifest |py| {
    data underlying: Arc<RwLock<TreeManifest>>;
    data pending_delete: RefCell<HashSet<RepoPathBuf>>;

    def __new__(
        _cls,
        store: ImplInto<Arc<dyn TreeStore + Send + Sync>>,
        node: Option<&PyBytes> = None
    ) -> PyResult<treemanifest> {
        let manifest_store = store.into();
        let underlying = match node {
            None => TreeManifest::ephemeral(manifest_store),
            Some(value) => TreeManifest::durable(manifest_store, pybytes_to_node(py, value)?),
        };
        treemanifest::create_instance(py, Arc::new(RwLock::new(underlying)), RefCell::new(HashSet::new()))
    }

    // Returns a new instance of treemanifest that contains the same data as the base.
    def copy(&self) -> PyResult<treemanifest> {
        treemanifest::create_instance(
            py,
            Arc::new(RwLock::new(self.underlying(py).read().clone())),
            self.pending_delete(py).clone()
        )
    }

    // Returns (node, flag) for a given `path` in the manifest.
    // When the `path` does not exist, it return a KeyError.
    def find(&self, path: PyPathBuf) -> PyResult<(PyBytes, String)> {
        // Some code... probably sparse profile related is asking find to grab
        // random invalid paths.
        let repo_path = match path.to_repo_path().map_pyerr(py) {
            Ok(value) => value,
            Err(_) => {
                let msg = format!(
                    "cannot find file '{}' in manifest",
                    path,
                );
                return Err(PyErr::new::<exc::KeyError, _>(py, msg))
            }
        };
        let tree = self.underlying(py).read();
        match tree.get_file(&repo_path).map_pyerr(py)? {
            None => {
                let msg = format!("cannot find file '{}' in manifest", repo_path);
                Err(PyErr::new::<exc::KeyError, _>(py, msg))
            }
            Some(file_metadata) => file_metadata_to_py_tuple(py, &file_metadata),
        }
    }

    def get(&self, path: PyPathBuf, default: Option<PyBytes> = None) -> PyResult<Option<PyBytes>> {
        let repo_path = path.to_repo_path().map_pyerr(py)?;
        let tree = self.underlying(py).read();
        let result = match tree.get_file(&repo_path).map_pyerr(py)? {
            None => None,
            Some(file_metadata) => Some(node_to_pybytes(py, file_metadata.hgid)),
        };
        Ok(result.or(default))
    }

    def flags(&self, path: PyPathBuf, default: Option<PyString> = None) -> PyResult<PyString> {
        let repo_path = path.to_repo_path().map_pyerr(py)?;
        let tree = self.underlying(py).read();
        let result = match tree.get_file(&repo_path).map_pyerr(py)? {
            None => None,
            Some(file_metadata) => Some(file_type_to_pystring(py, file_metadata.file_type)),
        };
        Ok(result.or(default).unwrap_or_else(|| PyString::new(py, "")))
    }

    def hasdir(&self, path: PyPathBuf) -> PyResult<bool> {
        let repo_path = path.to_repo_path().map_pyerr(py)?;
        let tree = self.underlying(py).read();
        let result = match tree.get(&repo_path).map_pyerr(py)? {
            Some(FsNodeMetadata::Directory(_)) => true,
            _ => false
        };
        Ok(result)
    }

    // Returns a list<path> for all files that match the predicate passed to the function.
    def walk(&self, pymatcher: PyObject) -> PyResult<Vec<PyPathBuf>> {
        let mut result = Vec::new();
        let tree = self.underlying(py);
        let matcher = extract_matcher(py, pymatcher)?;
        let files = py.allow_threads(move || -> Vec<_> {
            let tree = tree.read();
            tree.files(matcher).collect()
        });
        for entry in files.into_iter() {
            let file = entry.map_pyerr(py)?;
            result.push(file.path.into());
        }
        Ok(result)
    }

    /// Returns [(path, id)] for directories.
    def walkdirs(&self, pymatcher: PyObject) -> PyResult<Vec<(PyPathBuf, Option<PyBytes>)>> {
        let mut result = Vec::new();
        let tree = self.underlying(py);
        let matcher = extract_matcher(py, pymatcher)?;
        let dirs = py.allow_threads(move || -> Vec<_> {
            let tree = tree.read();
            tree.dirs(matcher).collect()
        });
        for entry in dirs.into_iter() {
            let dir = entry.map_pyerr(py)?;
            result.push((
                dir.path.into(),
                dir.hgid.map(|id| PyBytes::new(py, id.as_ref())),
            ));
        }
        Ok(result)
    }

    def listdir(&self, path: PyPathBuf) -> PyResult<Vec<PyPathBuf>> {
        let repo_path = path.to_repo_path().map_pyerr(py)?;
        let tree = self.underlying(py).read();
        let result = match tree.list(&repo_path).map_pyerr(py)? {
            manifest::List::NotFound | manifest::List::File => vec![],
            manifest::List::Directory(components) =>
                components.into_iter().map(|(component, _)|
                    component.into()
                ).collect()
        };
        Ok(result)
    }

    def text(&self, matcher: Option<PyObject> = None) -> PyResult<PyBytes> {
        let mut lines = Vec::new();
        let tree = self.underlying(py);
        let matcher = extract_option_matcher(py, matcher)?;
        let files = py.allow_threads(move || -> Vec<_> {
            let tree = tree.read();
            tree.files(matcher).collect()
        });
        for entry in files.into_iter() {
            let file = entry.map_pyerr(py)?;
            lines.push(format!(
                "{}\0{}{}\n",
                file.path,
                file.meta.hgid,
                file_type_to_str(file.meta.file_type)
            ));
        }
        lines.sort();
        // TODO: Optimize this so that the string does not get copied.
        Ok(PyBytes::new(py, lines.concat().as_bytes()))
    }

    def set(&self, path: PyPathBuf, binnode: &PyBytes, flag: &PyString) -> PyResult<PyNone> {
        // TODO: can the node and flag that are passed in be None?
        let tree = self.underlying(py);
        let repo_path_buf = path.to_repo_path_buf().map_pyerr(py)?;
        let node = pybytes_to_node(py, binnode)?;
        let file_type = pystring_to_file_type(py, flag)?;
        let file_metadata = FileMetadata::new(node, file_type);
        let to_delete = py.allow_threads(move || -> Result<HashSet<RepoPathBuf>> {
            let mut tree = tree.write();
            insert(&mut tree, repo_path_buf, file_metadata)
        }).map_pyerr(py)?;
        let mut pending_delete = self.pending_delete(py).borrow_mut();
        for path in to_delete.into_iter() {
            pending_delete.remove(&path);
        }
        Ok(PyNone)
    }

    def setflag(&self, path: PyPathBuf, flag: &PyString) -> PyResult<PyObject> {
        let tree = self.underlying(py);
        let repo_path_buf = path.to_repo_path_buf().map_pyerr(py)?;
        let file_type = pystring_to_file_type(py, flag)?;
        let file_metadata = match tree.read().get_file(&repo_path_buf).map_pyerr(py)? {
            None => {
                let msg = "cannot setflag on file that is not in manifest";
                return Err(PyErr::new::<exc::KeyError, _>(py, msg));
            }
            Some(mut file_metadata) => {
                file_metadata.file_type = file_type;
                file_metadata
            }
        };
        let to_delete = py.allow_threads(move || -> Result<HashSet<RepoPathBuf>> {
            let mut tree = tree.write();
            insert(&mut tree, repo_path_buf, file_metadata)
        }).map_pyerr(py)?;
        let mut pending_delete = self.pending_delete(py).borrow_mut();
        for path in to_delete.into_iter() {
            pending_delete.remove(&path);
        }
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
                    Some(node_to_pybytes(py, file_metadata.hgid)),
                    file_type_to_pystring(py, file_metadata.file_type)
                )
            }
        }

        let result = PyDict::new(py);
        let matcher: Arc<dyn Matcher + Sync + Send> = extract_option_matcher(py, matcher)?;
        let this_tree = self.underlying(py);
        let other_tree = other.underlying(py);

        let results: Vec<_> = py.allow_threads(move || -> Result<_> {
            manifest_tree::Diff::new(&this_tree.read(), &other_tree.read(), &matcher)?.collect()
        }).map_pyerr(py)?;
        for entry in results {
            let path = PyPathBuf::from(entry.path);
            let diff_left = convert_side_diff(py, entry.diff_type.left());
            let diff_right = convert_side_diff(py, entry.diff_type.right());
            result.set_item(py, path, (diff_left, diff_right))?;
        }
        Ok(result)
    }

    /// Find modified directories. Return [(path: str, exist_left: bool, exist_right: bool)].
    /// Modified directories are added, removed, or metadata changed (direct file or subdir added,
    /// removed, similar to when OS updates mtime of a directory). File content change does not
    /// modify its parent directory.
    def modifieddirs(&self, other: &treemanifest, matcher: Option<PyObject> = None) -> PyResult<Vec<(PyPathBuf, bool, bool)>> {
        let matcher: Arc<dyn Matcher + Sync + Send> = extract_option_matcher(py, matcher)?;
        let this_tree = self.underlying(py);
        let other_tree = other.underlying(py);
        let results = py.allow_threads(move || -> Result<_> {
            let this = this_tree.read();
            let other = other_tree.read();
            let modified_dirs = this.modified_dirs(&other, &matcher);
            modified_dirs.and_then(|v| v.collect::<Result<Vec<_>>>())
        }).map_pyerr(py)?;
        let results = results.into_iter().map(|i| (i.path.into(), i.left, i.right)).collect();
        Ok(results)
    }

    def filesnotin(
        &self,
        other: &treemanifest,
        matcher: Option<PyObject> = None
    ) -> PyResult<PyObject> {
        let mut result = pyset_new(py)?;
        let this_tree = self.underlying(py);
        let other_tree = other.underlying(py);
        let matcher: Arc<dyn Matcher + Sync + Send> = extract_option_matcher(py, matcher)?;

        let results: Vec<_> = py.allow_threads(move || -> Result<_> {
            manifest_tree::Diff::new(&this_tree.read(), &other_tree.read(), &matcher)?.collect()
        }).map_pyerr(py)?;
        for entry in results {
            match entry.diff_type {
                DiffType::LeftOnly(_) => {
                    pyset_add(py, &mut result, PyPathBuf::from(entry.path))?;
                }
                DiffType::RightOnly(_) | DiffType::Changed(_, _) => (),
            }
        }
        Ok(result)
    }

    def matches(&self, pymatcher: PyObject) -> PyResult<PyObject> {
        let flatmanifest = self.text(py, Some(pymatcher))?;
        let manifestmod = py.import("edenscm.manifest")?;
        let manifestdict = manifestmod.get(py, "manifestdict")?;
        manifestdict.call(py, (flatmanifest,), None)
    }

    def __setitem__(&self, path: PyPathBuf, binnode: &PyBytes) -> PyResult<()> {
        let tree = self.underlying(py);
        let repo_path_buf = path.to_repo_path_buf().map_pyerr(py)?;
        let node = pybytes_to_node(py, binnode)?;
        let file_metadata = match tree.read().get_file(&repo_path_buf).map_pyerr(py)? {
            None => FileMetadata::new(node, FileType::Regular),
            Some(mut file_metadata) => {
                file_metadata.hgid = node;
                file_metadata
            }
        };
        let to_delete = py.allow_threads(move || -> Result<HashSet<RepoPathBuf>> {
            let mut tree = tree.write();
            insert(&mut tree, repo_path_buf, file_metadata)
        }).map_pyerr(py)?;
        let mut pending_delete = self.pending_delete(py).borrow_mut();
        for path in to_delete.into_iter() {
            pending_delete.remove(&path);
        }
        Ok(())
    }

    def __delitem__(&self, path: PyPathBuf) -> PyResult<()> {
        let mut tree = self.underlying(py).write();
        let repo_path = path.to_repo_path().map_pyerr(py)?;
        tree.remove(&repo_path).map_pyerr(py)?;
        let mut pending_delete = self.pending_delete(py).borrow_mut();
        pending_delete.remove(repo_path);
        Ok(())
    }

    def __getitem__(&self, path: PyPathBuf) -> PyResult<PyBytes> {
        let repo_path = path.to_repo_path().map_pyerr(py)?;
        let tree = self.underlying(py).read();
        match tree.get_file(&repo_path).map_pyerr(py)? {
            Some(file_metadata) => Ok(node_to_pybytes(py, file_metadata.hgid)),
            None => Err(PyErr::new::<exc::KeyError, _>(py, format!("file {} not found", path))),
        }
    }

    def keys(&self) -> PyResult<Vec<PyPathBuf>> {
        let mut result = Vec::new();
        let tree = self.underlying(py);
        let files = py.allow_threads(move || -> Vec<_> {
            let tree = tree.read();
            tree.files(AlwaysMatcher::new()).collect()
        });
        for entry in files {
            let file = entry.map_pyerr(py)?;
            result.push(file.path.into());
        }
        Ok(result)
    }

    // iterator stuff

    def __contains__(&self, path: PyPathBuf) -> PyResult<bool> {
        let repo_path = path.to_repo_path().map_pyerr(py)?;
        let tree = self.underlying(py).read();
        match tree.get_file(&repo_path).map_pyerr(py)? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    def __iter__(&self) -> PyResult<PyObject> {
        let mut result = Vec::new();
        let tree = self.underlying(py);
        let files = py.allow_threads(move || -> Vec<_> {
            let tree = tree.read();
            tree.files(AlwaysMatcher::new()).collect()
        });
        for entry in files {
            let file = entry.map_pyerr(py)?;
            result.push(PyPathBuf::from(file.path));
        }
        vec_to_iter(py, result)
    }

    def iteritems(&self) -> PyResult<PyObject> {
        self.items(py)
    }

    def items(&self) -> PyResult<PyObject> {
        let mut result = Vec::new();
        let tree = self.underlying(py);
        let files = py.allow_threads(move || -> Vec<_> {
            let tree = tree.read();
            tree.files(AlwaysMatcher::new()).collect()
        });
        for entry in files {
            let file = entry.map_pyerr(py)?;
            let tuple = (
                PyPathBuf::from(file.path),
                node_to_pybytes(py, file.meta.hgid),
            );
            result.push(tuple);
        }
        vec_to_iter(py, result)
    }

    def iterkeys(&self) -> PyResult<PyObject> {
        let mut result = Vec::new();
        let tree = self.underlying(py);
        let files = py.allow_threads(move || -> Vec<_> {
            let tree = tree.read();
            tree.files(AlwaysMatcher::new()).collect()
        });
        for entry in files {
            let file = entry.map_pyerr(py)?;
            result.push(PyPathBuf::from(file.path));
        }
        vec_to_iter(py, result)
    }

    def finalize(
        &self,
        p1tree: Option<&treemanifest> = None,
        p2tree: Option<&treemanifest> = None
    ) -> PyResult<Vec<PyTuple>> {
        let pending_delete = self.pending_delete(py).borrow();
        if !pending_delete.is_empty() {
            return Err(PyErr::new::<exc::RuntimeError, _>(
                py,
                format!(
                    "Error finalizing manifest. Invalid state: \
                    expecting deletion commands for the following paths: {:?}",
                    pending_delete
                )
            ));
        }
        let mut result = Vec::new();
        let mut tree = self.underlying(py).write();
        let mut parents = vec!();
        if let Some(m1) = p1tree {
            parents.push(m1.underlying(py).read());
        }
        if let Some(m2) = p2tree {
            parents.push(m2.underlying(py).read());
        }
        let entries = tree.finalize(
            parents.iter().map(|x| x.deref()).collect()
        ).map_pyerr(py)?;
        for entry in entries {
            let (repo_path, node, raw, p1node, p2node) = entry;
            let tuple = PyTuple::new(
                py,
                &[
                    PyPathBuf::from(repo_path).to_py_object(py).into_object(),
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

    /// flush() -> node.
    /// Write pending trees to store. Return root node.
    /// Only works for git store. Use finalize() for hg store instead.
    def flush(&self) -> PyResult<PyBytes> {
        let mut tree = self.underlying(py).write();
        let hgid = tree.flush().map_pyerr(py)?;
        Ok(PyBytes::new(py, hgid.as_ref()))
    }

});

impl treemanifest {
    pub fn get_underlying(&self, py: Python) -> Arc<RwLock<TreeManifest>> {
        self.underlying(py).clone()
    }
}

pub fn subdir_diff(
    py: Python,
    store: ImplInto<Arc<dyn TreeStore + Send + Sync>>,
    path: PyPathBuf,
    binnode: &PyBytes,
    other_binnodes: &PyList,
    depth: i32,
) -> PyResult<PyObject> {
    let manifest_store = store.into();
    let mut others = vec![];
    for pybytes in other_binnodes.iter(py) {
        others.push(pybytes_to_node(py, &pybytes.extract(py)?)?);
    }
    let diff = manifest_tree::compat_subtree_diff(
        manifest_store,
        path.to_repo_path().map_pyerr(py)?,
        pybytes_to_node(py, binnode)?,
        others,
        depth,
    )
    .map_pyerr(py)?;
    let mut result = vec![];
    for (path, node, others, bytes) in diff {
        use types::HgId;
        let p1 = others.get(0).unwrap_or(HgId::null_id()).clone();
        let p2 = others.get(1).unwrap_or(HgId::null_id()).clone();
        let tuple = PyTuple::new(
            py,
            &[
                PyPathBuf::from(path).to_py_object(py).into_object(),
                node_to_pybytes(py, node).into_object(),
                PyBytes::new(py, &bytes).into_object(),
                node_to_pybytes(py, p1).into_object(),
                node_to_pybytes(py, p2).into_object(),
            ],
        );
        result.push(tuple);
    }
    vec_to_iter(py, result)
}

pub fn prefetch(
    py: Python,
    store: ImplInto<Arc<dyn TreeStore + Send + Sync>>,
    node: &PyBytes,
    path: PyPathBuf,
    depth: Option<usize>,
) -> PyResult<PyNone> {
    let store = store.into();
    let node = pybytes_to_node(py, node)?;
    let repo_path_buf = path.to_repo_path_buf().map_pyerr(py)?;
    let key = Key::new(repo_path_buf, node);
    manifest_tree::prefetch(store, key, depth).map_pyerr(py)?;
    Ok(PyNone)
}

fn insert(
    tree: &mut TreeManifest,
    path: RepoPathBuf,
    file_metadata: FileMetadata,
) -> Result<HashSet<RepoPathBuf>> {
    let mut to_delete = HashSet::new();
    let insert_error = match tree.insert(path, file_metadata) {
        Ok(()) => return Ok(to_delete),
        Err(error) => match error.downcast::<manifest_tree::InsertError>() {
            Ok(insert_error) => insert_error,
            Err(err) => return Err(err),
        },
    };
    let path = insert_error.path;
    match insert_error.source {
        manifest_tree::InsertErrorCause::ParentFileExists(file_path) => {
            tree.remove(&file_path)?;
            to_delete.insert(file_path);
        }
        manifest_tree::InsertErrorCause::DirectoryExistsForPath => {
            let files: Vec<File> = tree
                .files(TreeMatcher::from_rules([format!("{}/**", path)].iter())?)
                .collect::<Result<_>>()?;
            for file in files {
                tree.remove(&file.path)?;
                to_delete.insert(file.path);
            }
        }
    }
    tree.insert(path, file_metadata)?;
    Ok(to_delete)
}

fn vec_to_iter<T: ToPyObject>(py: Python, items: Vec<T>) -> PyResult<PyObject> {
    let list: PyList = items.into_py_object(py);
    list.into_object().call_method(py, "__iter__", NoArgs, None)
}

fn file_metadata_to_py_tuple(
    py: Python,
    file_metadata: &FileMetadata,
) -> PyResult<(PyBytes, String)> {
    let node = PyBytes::new(py, file_metadata.hgid.as_ref());
    let flag = file_type_to_str(file_metadata.file_type).to_string();
    Ok((node, flag))
}

fn pybytes_to_node(py: Python, pybytes: &PyBytes) -> PyResult<Node> {
    Node::from_slice(pybytes.data(py)).map_pyerr(py)
}

fn node_to_pybytes(py: Python, node: Node) -> PyBytes {
    PyBytes::new(py, node.as_ref())
}

fn pystring_to_file_type(py: Python, pystring: &PyString) -> PyResult<FileType> {
    match pystring.to_string_lossy(py).borrow() {
        "x" => Ok(FileType::Executable),
        "l" => Ok(FileType::Symlink),
        "m" => Ok(FileType::GitSubmodule),
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
        FileType::GitSubmodule => "m",
    }
}
