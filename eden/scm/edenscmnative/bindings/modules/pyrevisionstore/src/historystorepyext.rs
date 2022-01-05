/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Result;
use cpython::PyBytes;
use cpython::PyIterator;
use cpython::PyList;
use cpython::PyObject;
use cpython::PyResult;
use cpython::PyTuple;
use cpython::Python;
use cpython::PythonObject;
use cpython::ToPyObject;
use cpython_ext::PyNone;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use revisionstore::HgIdHistoryStore;
use revisionstore::HgIdMutableHistoryStore;
use revisionstore::RemoteHistoryStore;
use revisionstore::StoreKey;
use revisionstore::ToKeys;
use types::Key;
use types::NodeInfo;

use crate::pythonutil::from_key;
use crate::pythonutil::from_key_to_tuple;
use crate::pythonutil::from_tuple_to_key;
use crate::pythonutil::key_error;
use crate::pythonutil::to_key;
use crate::pythonutil::to_node;
use crate::pythonutil::to_path;

pub trait HgIdHistoryStorePyExt {
    fn get_missing_py(&self, py: Python, keys: &mut PyIterator) -> PyResult<PyList>;
    fn get_node_info_py(&self, py: Python, name: &PyPathBuf, node: &PyBytes) -> PyResult<PyTuple>;
    fn refresh_py(&self, py: Python) -> PyResult<PyNone>;
}

pub trait IterableHgIdHistoryStorePyExt {
    fn iter_py(&self, py: Python) -> PyResult<Vec<PyTuple>>;
}

pub trait HgIdMutableHistoryStorePyExt: HgIdHistoryStorePyExt {
    fn add_py(
        &self,
        py: Python,
        name: &PyPathBuf,
        node: &PyBytes,
        p1: &PyBytes,
        p2: &PyBytes,
        linknode: &PyBytes,
        copyfrom: Option<&PyPathBuf>,
    ) -> PyResult<PyObject>;
    fn flush_py(&self, py: Python) -> PyResult<Option<Vec<PyPathBuf>>>;
}

pub trait RemoteHistoryStorePyExt: RemoteHistoryStore {
    fn prefetch_py(&self, py: Python, keys: PyList) -> PyResult<PyObject>;
}

impl<T: HgIdHistoryStore + ?Sized> HgIdHistoryStorePyExt for T {
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
                StoreKey::Content(_, _) => {
                    return Err(format_err!("Unsupported key: {:?}", key)).map_pyerr(py);
                }
            }
        }

        Ok(results)
    }

    fn get_node_info_py(&self, py: Python, name: &PyPathBuf, node: &PyBytes) -> PyResult<PyTuple> {
        let key = to_key(py, name, node)?;
        let info = py
            .allow_threads(|| self.get_node_info(&key))
            .map_pyerr(py)?
            .ok_or_else(|| key_error(py, &StoreKey::hgid(key.clone())))?;

        Ok(from_node_info(py, &key, &info))
    }

    fn refresh_py(&self, py: Python) -> PyResult<PyNone> {
        self.refresh().map_pyerr(py)?;
        Ok(PyNone)
    }
}

fn from_node_info(py: Python, key: &Key, info: &NodeInfo) -> PyTuple {
    (
        PyBytes::new(py, info.parents[0].hgid.as_ref()),
        PyBytes::new(py, info.parents[1].hgid.as_ref()),
        PyBytes::new(py, info.linknode.as_ref().as_ref()),
        if key.path != info.parents[0].path {
            if info.parents[0].path.is_empty() {
                Python::None(py)
            } else {
                PyPathBuf::from(info.parents[0].path.clone())
                    .to_py_object(py)
                    .into_object()
            }
        } else {
            Python::None(py)
        },
    )
        .into_py_object(py)
}

fn to_node_info(
    py: Python,
    name: &PyPathBuf,
    p1: &PyBytes,
    p2: &PyBytes,
    linknode: &PyBytes,
    copyfrom: Option<&PyPathBuf>,
) -> PyResult<NodeInfo> {
    // Not only can copyfrom be None, it can also be an empty string. In both cases that means that
    // `name` should be used.
    let copyfrom = copyfrom.unwrap_or(name);
    let p1path = if copyfrom.as_path().as_os_str().is_empty() {
        name
    } else {
        copyfrom
    };
    let p1node = to_node(py, p1);
    let p2node = to_node(py, p2);

    let parents = if p1node.is_null() {
        Default::default()
    } else if p2node.is_null() {
        let p1 = Key::new(to_path(py, p1path)?, p1node);
        let p2 = Key::default();
        [p1, p2]
    } else {
        let p1 = Key::new(to_path(py, p1path)?, p1node);
        let p2 = Key::new(to_path(py, name)?, p2node);
        [p1, p2]
    };

    let linknode = to_node(py, linknode);
    Ok(NodeInfo { parents, linknode })
}

impl<T: ToKeys + HgIdHistoryStore + ?Sized> IterableHgIdHistoryStorePyExt for T {
    fn iter_py(&self, py: Python) -> PyResult<Vec<PyTuple>> {
        let iter = py.allow_threads(|| self.to_keys()).into_iter().map(|res| {
            let key = res?;
            let node_info = py.allow_threads(|| self.get_node_info(&key))?.unwrap();
            let (name, node) = from_key(py, &key);
            let copyfrom = if key.path != node_info.parents[0].path {
                if node_info.parents[0].path.is_empty() {
                    PyPathBuf::from(String::from(""))
                } else {
                    PyPathBuf::from(node_info.parents[0].path.as_repo_path())
                }
            } else {
                PyPathBuf::from(String::from(""))
            };
            let tuple = (
                name.to_py_object(py).into_object(),
                node.into_object(),
                PyBytes::new(py, node_info.parents[0].hgid.as_ref()),
                PyBytes::new(py, node_info.parents[1].hgid.as_ref()),
                PyBytes::new(py, node_info.linknode.as_ref().as_ref()),
                copyfrom.to_py_object(py).into_object(),
            )
                .into_py_object(py);
            Ok(tuple)
        });
        iter.collect::<Result<Vec<PyTuple>>>().map_pyerr(py)
    }
}

impl<T: HgIdMutableHistoryStore + ?Sized> HgIdMutableHistoryStorePyExt for T {
    fn add_py(
        &self,
        py: Python,
        name: &PyPathBuf,
        node: &PyBytes,
        p1: &PyBytes,
        p2: &PyBytes,
        linknode: &PyBytes,
        copyfrom: Option<&PyPathBuf>,
    ) -> PyResult<PyObject> {
        let key = to_key(py, name, node)?;
        let nodeinfo = to_node_info(py, name, p1, p2, linknode, copyfrom)?;
        py.allow_threads(|| self.add(&key, &nodeinfo))
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

impl<T: RemoteHistoryStore + ?Sized> RemoteHistoryStorePyExt for T {
    fn prefetch_py(&self, py: Python, keys: PyList) -> PyResult<PyObject> {
        let keys = keys
            .iter(py)
            .map(|tuple| Ok(StoreKey::from(from_tuple_to_key(py, &tuple)?)))
            .collect::<PyResult<Vec<StoreKey>>>()?;
        py.allow_threads(|| self.prefetch(&keys)).map_pyerr(py)?;
        Ok(Python::None(py))
    }
}
