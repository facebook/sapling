// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::path::PathBuf;

use cpython::{
    FromPyObject, NoArgs, ObjectProtocol, PyBytes, PyObject, PyResult, PyTuple, Python,
    PythonObject,
};
use failure::{format_err, Fallible};

use encoding::local_bytes_to_path;
use revisionstore::MutableHistoryStore;
use types::{Key, NodeInfo};

use crate::revisionstore::pyerror::pyerr_to_error;
use crate::revisionstore::pythonutil::{from_key, to_pyerr};

pub struct PythonMutableHistoryPack {
    py_historypack: PyObject,
}

// All accesses are protected by the GIL, so it's thread safe. This is required because it is
// eventually stored on the `datastore` python class and Rust CPython requires that stored members
// implement Send.
unsafe impl Send for PythonMutableHistoryPack {}

impl PythonMutableHistoryPack {
    pub fn new(py_historypack: PyObject) -> PyResult<Self> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let py_type = py_historypack.get_type(py);
        let name = py_type.name(py);
        if name != "mutablehistorypack" {
            Err(to_pyerr(
                py,
                &format_err!(
                    "A 'mutablehistorypack' object was expected but got a '{}' object",
                    name
                ),
            ))
        } else {
            Ok(PythonMutableHistoryPack { py_historypack })
        }
    }

    fn build_add_tuple(&self, py: Python, key: &Key, info: &NodeInfo) -> PyTuple {
        let (py_name, py_node) = from_key(py, key);
        let py_linknode = PyBytes::new(py, info.linknode.as_ref());
        let py_copyfrom = if info.parents[0].path == key.path {
            PyBytes::new(py, b"")
        } else {
            PyBytes::new(py, info.parents[0].path.as_byte_slice())
        };
        let py_p1 = PyBytes::new(py, info.parents[0].node.as_ref());
        let py_p2 = PyBytes::new(py, info.parents[1].node.as_ref());

        PyTuple::new(
            py,
            &vec![
                py_name.into_object(),
                py_node.into_object(),
                py_p1.into_object(),
                py_p2.into_object(),
                py_linknode.into_object(),
                py_copyfrom.into_object(),
            ],
        )
    }
}

impl MutableHistoryStore for PythonMutableHistoryPack {
    fn add(&mut self, key: &Key, info: &NodeInfo) -> Fallible<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let py_tuple = self.build_add_tuple(py, key, info);
        self.py_historypack
            .call_method(py, "add", py_tuple, None)
            .map_err(|e| pyerr_to_error(py, e))?;
        Ok(())
    }

    fn flush(&mut self) -> Fallible<Option<PathBuf>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let py_path = self
            .py_historypack
            .call_method(py, "flush", NoArgs, None)
            .map_err(|e| pyerr_to_error(py, e))?;
        let py_path = PyBytes::extract(py, &py_path).map_err(|e| pyerr_to_error(py, e))?;
        Ok(Some(local_bytes_to_path(py_path.data(py))?.into_owned()))
    }

    fn close(self) -> Fallible<Option<PathBuf>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let py_path = self
            .py_historypack
            .call_method(py, "close", NoArgs, None)
            .map_err(|e| pyerr_to_error(py, e))?;
        let py_path = PyBytes::extract(py, &py_path).map_err(|e| pyerr_to_error(py, e))?;
        Ok(Some(local_bytes_to_path(py_path.data(py))?.into_owned()))
    }
}
