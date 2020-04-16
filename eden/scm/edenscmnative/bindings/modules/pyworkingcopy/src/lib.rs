/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;

use anyhow::Error;
use cpython::*;

use cpython_ext::PyPathBuf;
use pypathmatcher::UnsafePythonMatcher;
use workingcopy::walker::{WalkError, Walker};

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "workingcopy"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<walker>(py)?;
    Ok(m)
}

py_class!(class walker |py| {
    data walker: RefCell<Walker<UnsafePythonMatcher>>;
    data _errors: RefCell<Vec<Error>>;
    def __new__(_cls, root: PyPathBuf, pymatcher: PyObject, include_directories: bool) -> PyResult<walker> {
        let matcher = UnsafePythonMatcher::new(pymatcher);
        walker::create_instance(py, RefCell::new(Walker::new(root.to_path_buf(), matcher, include_directories)), RefCell::new(Vec::new()))
    }

    def __iter__(&self) -> PyResult<Self> {
        Ok(self.clone_ref(py))
    }

    def __next__(&self) -> PyResult<Option<PyPathBuf>> {
        loop {
            match self.walker(py).borrow_mut().next() {
                Some(Ok(path)) => {
                    return Ok(Some(PyPathBuf::from(path.as_ref())))
                },
                Some(Err(e)) => {
                    self._errors(py).borrow_mut().push(e)
                },
                None => return Ok(None),
            };
        }
    }

    def errors(&self) -> PyResult<Vec<(cpython_ext::Str, cpython_ext::Str)>> {
        Ok(self._errors(py).borrow().iter().map(|e| match e.downcast_ref::<WalkError>() {
            Some(e) => (e.filename().into(), e.message().into()),
            None => ("unknown".to_string().into(), e.to_string().into()),
        }).collect::<Vec<(cpython_ext::Str, cpython_ext::Str)>>())
    }

});
