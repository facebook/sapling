/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::PyPathBuf;
use pathmatcher::Matcher;
use pyconfigparser::config as PyConfig;
use pymanifest::treemanifest;
use pypathmatcher::extract_matcher;
use pypathmatcher::extract_option_matcher;
use pytreestate::treestate;
use storemodel::ReadFileContents;
use workingcopy::walker::WalkError;
use workingcopy::walker::Walker;

type ArcReadFileContents = Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "workingcopy"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<walker>(py)?;
    m.add_class::<status>(py)?;
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

py_class!(class status |py| {
    @staticmethod
    def status(
        pyroot: PyPathBuf,
        pymanifest: treemanifest,
        pystore: ImplInto<ArcReadFileContents>,
        pytreestate: treestate,
        last_write: u32,
        pymatcher: Option<PyObject>,
        listunknown: bool,
        filesystem: &str,
        config: PyConfig,
    ) -> PyResult<PyObject> {
        let root = pyroot.to_path_buf();
        let manifest = pymanifest.get_underlying(py);
        let store = pystore.into();
        let last_write = SystemTime::UNIX_EPOCH.checked_add(
            Duration::from_secs(last_write.into())).ok_or_else(|| anyhow!("Failed to convert {} to SystemTime", last_write)
        ).map_pyerr(py)?;
        let matcher = extract_option_matcher(py, pymatcher)?;
        let filesystem = match filesystem {
            "normal" => {
                workingcopy::filesystem::FileSystemType::Normal
            },
            "watchman" => {
                workingcopy::filesystem::FileSystemType::Watchman
            },
            "eden" => {
                workingcopy::filesystem::FileSystemType::Eden
            },
            _ => return Err(anyhow!("Unsupported filesystem type: {}", filesystem)).map_pyerr(py),
        };

        let state = pytreestate.get_state(py);
        let mut option = state.lock();
        let treestate = option.take().expect("TreeState is never taken outside of lock");

        let config = config.get_cfg(py);
        let (treestate, status) = py.allow_threads(|| workingcopy::status::status(
            root,
            filesystem,
            manifest,
            store,
            treestate,
            last_write,
            matcher,
            listunknown,
            &config,
        ));

        option.replace(treestate);
        let status = status.map_pyerr(py)?;
        pystatus::to_python_status(py, &status)
    }
});
