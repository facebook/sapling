// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![allow(non_camel_case_types)]

use std::cell::RefCell;

use cpython::*;

use ::nodemap::nodemap::NodeMap;
use encoding::local_bytes_to_path;
use types::node::Node;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "nodemap"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<nodemap>(py)?;
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
});
