/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;

use cpython::*;

use ::nodemap::{NodeMap, NodeSet, Repair};
use cpython_ext::Bytes;
use cpython_ext::ResultPyErrExt;
use encoding::local_bytes_to_path;
use types::node::Node;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "nodemap"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<nodemap>(py)?;
    m.add_class::<nodeset>(py)?;
    Ok(m)
}

py_class!(class nodemap |py| {
    data log: RefCell<NodeMap>;

    def __new__(_cls, path: &PyBytes) -> PyResult<nodemap> {
        let path = local_bytes_to_path(path.data(py))
            .map_err(|e| PyErr::new::<exc::ValueError, _>(py, format!("{}", e)))?;
        let nodemap = NodeMap::open(path)
            .map_err(|e| PyErr::new::<exc::RuntimeError, _>(py, format!("{}", e)))?;
        nodemap::create_instance(py, RefCell::new(nodemap))
    }

    def add(&self, first: &PyBytes, second: &PyBytes) -> PyResult<PyObject> {
        let first = Node::from_slice(first.data(py))
            .map_err(|e| PyErr::new::<exc::ValueError, _>(py, format!("{}", e)))?;
        let second = Node::from_slice(second.data(py))
            .map_err(|e| PyErr::new::<exc::ValueError, _>(py, format!("{}", e)))?;

        let cell = self.log(py);
        let mut log = cell.borrow_mut();
        log.add(&first, &second)
            .map_err(|e| PyErr::new::<exc::RuntimeError, _>(py, format!("{}", e)))?;

        Ok(py.None())
    }

    def flush(&self) -> PyResult<PyObject> {
        self.log(py).borrow_mut().flush()
            .map_err(|e| PyErr::new::<exc::RuntimeError, _>(py, format!("{}", e)))?;
        Ok(py.None())
    }

    def lookupbyfirst(&self, first: &PyBytes) -> PyResult<PyObject> {
        let first = Node::from_slice(first.data(py))
            .map_err(|e| PyErr::new::<exc::ValueError, _>(py, format!("{}", e)))?;
        Ok(self.log(py).borrow().lookup_by_first(&first)
            .map_err(|e| PyErr::new::<exc::RuntimeError, _>(py, format!("{}", e)))?
            .map_or(py.None(), |node| PyBytes::new(py, node.as_ref()).into_object()))
    }

    def lookupbysecond(&self, second: &PyBytes) -> PyResult<PyObject> {
        let second = Node::from_slice(second.data(py))
            .map_err(|e| PyErr::new::<exc::ValueError, _>(py, format!("{}", e)))?;
        Ok(self.log(py).borrow().lookup_by_second(&second)
            .map_err(|e| PyErr::new::<exc::RuntimeError, _>(py, format!("{}", e)))?
            .map_or(py.None(), |node| PyBytes::new(py, node.as_ref()).into_object()))
    }

    def items(&self) -> PyResult<Vec<(PyBytes, PyBytes)>> {
        let log = self.log(py).borrow();
        let iter = log.iter()
            .map_err(|e|  PyErr::new::<exc::RuntimeError, _>(py, format!("{}", e)))?;
        let keys = iter
            .map(|result| result.map(|keys| {
                let (first, second) = keys;
                (PyBytes::new(py, first.as_ref()),
                 PyBytes::new(py, second.as_ref()))
            }))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e|  PyErr::new::<exc::RuntimeError, _>(py, format!("{}", e)))?;
        Ok(keys)
    }

    @staticmethod
    def repair(path: &str) -> PyResult<PyUnicode> {
        py.allow_threads(|| NodeMap::repair(path)).map_pyerr(py).map(|s| PyUnicode::new(py, &s))
    }
});

py_class!(class nodeset |py| {
    data set: RefCell<NodeSet>;

    def __new__(_cls, path: &PyBytes) -> PyResult<Self> {
        let path = local_bytes_to_path(path.data(py)).map_pyerr(py)?;
        let nodeset = NodeSet::open(path).map_pyerr(py)?;
        Self::create_instance(py, RefCell::new(nodeset))
    }

    def add(&self, node: &PyBytes) -> PyResult<PyObject> {
        let node = Node::from_slice(node.data(py)).map_pyerr(py)?;
        let set = self.set(py);
        let mut set = set.borrow_mut();
        set.add(&node).map_pyerr(py)?;
        Ok(py.None())
    }

    def flush(&self) -> PyResult<PyObject> {
        self.set(py).borrow_mut().flush().map_pyerr(py)?;
        Ok(py.None())
    }

    def __contains__(&self, node: &PyBytes) -> PyResult<bool> {
        let node = Node::from_slice(node.data(py)).map_pyerr(py)?;
        Ok(self.set(py).borrow().contains(&node).map_pyerr(py)?)
    }

    def items(&self) -> PyResult<Vec<Bytes>> {
        let set = self.set(py).borrow();
        let nodes = set.iter()
            .map(|node| node.map(|node| Bytes::from(node.as_ref().to_vec())))
            .collect::<Result<Vec<Bytes>, _>>()
            .map_pyerr(py)?;
        Ok(nodes)
    }

    @staticmethod
    def repair(path: &str) -> PyResult<PyUnicode> {
        NodeSet::repair(path).map_pyerr(py).map(|s| PyUnicode::new(py, &s))
    }
});
