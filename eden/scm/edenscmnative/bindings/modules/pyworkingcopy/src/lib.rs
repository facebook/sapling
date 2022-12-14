/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

extern crate workingcopy as rsworkingcopy;

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;

use anyhow::anyhow;
use anyhow::Error;
use cpython::*;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::PyPathBuf;
use io::IO;
use parking_lot::RwLock;
use pathmatcher::Matcher;
use pyconfigloader::config;
use pypathmatcher::extract_matcher;
use pypathmatcher::extract_option_matcher;
use pytreestate::treestate;
use rsworkingcopy::walker::WalkError;
use rsworkingcopy::walker::Walker;
use rsworkingcopy::workingcopy::WorkingCopy;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "workingcopy"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<walker>(py)?;
    m.add_class::<workingcopy>(py)?;
    Ok(m)
}

py_class!(class walker |py| {
    data inner: RefCell<Walker<Arc<dyn Matcher + Sync + Send>>>;
    data _errors: RefCell<Vec<Error>>;
    def __new__(
        _cls,
        root: PyPathBuf,
        dot_dir: String,
        pymatcher: PyObject,
        include_directories: bool,
        thread_count: u8,
    ) -> PyResult<walker> {
        let matcher = extract_matcher(py, pymatcher)?;
        let walker = Walker::new(
            root.to_path_buf(),
            dot_dir,
            matcher,
            include_directories,
            thread_count,
        ).map_pyerr(py)?;
        walker::create_instance(py, RefCell::new(walker), RefCell::new(Vec::new()))
    }

    def __iter__(&self) -> PyResult<Self> {
        Ok(self.clone_ref(py))
    }

    def __next__(&self) -> PyResult<Option<PyPathBuf>> {
        loop {
            match self.inner(py).borrow_mut().next() {
                Some(Ok(path)) => return Ok(Some(PyPathBuf::from(path.as_ref()))),
                Some(Err(e)) => self._errors(py).borrow_mut().push(e),
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

py_class!(pub class workingcopy |py| {
    data inner: Arc<RwLock<WorkingCopy>>;

    def treestate(&self) -> PyResult<treestate> {
        treestate::create_instance(py, self.inner(py).read().treestate())
    }

    def status(
        &self,
        pymatcher: Option<PyObject>,
        lastwrite: u32,
        config: &config,
    ) -> PyResult<PyObject> {
        let wc = self.inner(py).write();
        let matcher = extract_option_matcher(py, pymatcher)?;
        let last_write = SystemTime::UNIX_EPOCH.checked_add(
            Duration::from_secs(lastwrite.into())).ok_or_else(|| anyhow!("Failed to convert {} to SystemTime", lastwrite)
        ).map_pyerr(py)?;
        let io = IO::main().map_pyerr(py)?;
        let config = config.get_cfg(py);
        pystatus::to_python_status(py,
            &py.allow_threads(|| {
                wc.status(matcher, last_write, &config, &io)
            }).map_pyerr(py)?
        )
    }
});
