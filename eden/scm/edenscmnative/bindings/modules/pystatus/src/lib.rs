/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use ::status::Status;
use ::status::StatusBuilder;
use cpython::*;
use cpython_ext::ExtractInnerRef;
use cpython_ext::ResultPyErrExt;
use types::RepoPathBuf;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "status"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<status>(py)?;
    Ok(m)
}

py_class!(pub class status |py| {
    data inner: Status;

    def __new__(
        _cls,
        python_status: PyObject,
    ) -> PyResult<status> {
        let modified = python_status.getattr(py, "modified")?;
        let added = python_status.getattr(py, "added")?;
        let removed = python_status.getattr(py, "removed")?;
        let deleted = python_status.getattr(py, "deleted")?;
        let unknown = python_status.getattr(py, "unknown")?;
        let ignored = python_status.getattr(py, "ignored")?;
        let clean = python_status.getattr(py, "clean")?;

        let builder = StatusBuilder::new()
            .modified(from_python_file_list(py, modified)?)
            .added(from_python_file_list(py, added)?)
            .removed(from_python_file_list(py, removed)?)
            .deleted(from_python_file_list(py, deleted)?)
            .unknown(from_python_file_list(py, unknown)?)
            .ignored(from_python_file_list(py, ignored)?)
            .clean(from_python_file_list(py, clean)?);

        status::create_instance(py, builder.build())
    }

    def __str__(&self) -> PyResult<PyString> {
        Ok(PyString::new(py, &self.inner(py).to_string()))
    }
});

impl ExtractInnerRef for status {
    type Inner = Status;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.inner(py)
    }
}

fn from_python_file_list(py: Python, list: PyObject) -> PyResult<Vec<RepoPathBuf>> {
    let list: PyList = list.cast_into(py)?;
    let mut files = vec![];
    for file in list.iter(py) {
        let file: PyString = file.cast_into(py)?;
        let file = file.to_string(py)?;
        let file = RepoPathBuf::from_string(file.to_string()).map_pyerr(py)?;
        files.push(file);
    }
    Ok(files)
}
