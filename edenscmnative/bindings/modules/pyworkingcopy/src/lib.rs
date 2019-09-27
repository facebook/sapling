// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![allow(non_camel_case_types)]

use std::cell::RefCell;

use cpython::*;

use cpython_failure::ResultPyErrExt;
use encoding::{local_bytes_to_path, repo_path_to_local_bytes};
use pypathmatcher::UnsafePythonMatcher;
use workingcopy::Walker;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "workingcopy"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<walker>(py)?;
    Ok(m)
}

py_class!(class walker |py| {
    data walker: RefCell<Walker<UnsafePythonMatcher>>;
    def __new__(_cls, root: PyBytes, pymatcher: PyObject) -> PyResult<walker> {
        let root = local_bytes_to_path(root.data(py))
            .map_pyerr::<exc::RuntimeError>(py)?
            .to_path_buf();
        let matcher = UnsafePythonMatcher::new(pymatcher);
        walker::create_instance(py, RefCell::new(Walker::new(root, matcher)))
    }

    def __iter__(&self) -> PyResult<Self> {
        Ok(self.clone_ref(py))
    }

    def __next__(&self) -> PyResult<Option<PyBytes>> {
        let item = match self.walker(py).borrow_mut().next() {
            Some(path) => Some(PyBytes::new(py, repo_path_to_local_bytes(path.map_pyerr::<exc::RuntimeError>(py)?.as_ref()))),
            None => None,
        };
        Ok(item)
    }

});
