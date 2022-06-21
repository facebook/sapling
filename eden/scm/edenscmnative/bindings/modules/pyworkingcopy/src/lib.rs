/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::PyPathBuf;
use pathmatcher::Matcher;
use pymanifest::treemanifest;
use pypathmatcher::extract_matcher;
use pytreestate::treestate;
use storemodel::ReadFileContents;
use types::RepoPathBuf;
use workingcopy::filesystem::ChangeType;
use workingcopy::filesystem::PendingChangeResult;
use workingcopy::filesystem::PendingChanges;
use workingcopy::filesystem::PhysicalFileSystem;
use workingcopy::walker::WalkError;
use workingcopy::walker::Walker;
use workingcopy::watchman::watchman::Watchman;

type ArcReadFileContents = Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "workingcopy"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<walker>(py)?;
    m.add_class::<pendingchanges>(py)?;
    m.add_class::<physicalfilesystem>(py)?;
    m.add_class::<watchman>(py)?;
    m.add_class::<status>(py)?;
    Ok(m)
}

py_class!(class physicalfilesystem |py| {
    data filesystem: RefCell<PhysicalFileSystem>;

    def __new__(_cls, root: PyPathBuf) -> PyResult<physicalfilesystem> {
        physicalfilesystem::create_instance(py, RefCell::new(PhysicalFileSystem::new(root.to_path_buf()).map_pyerr(py)?))
    }

    def pendingchanges(
        &self,
        pymanifest: treemanifest,
        pystore: ImplInto<ArcReadFileContents>,
        pytreestate: treestate,
        pymatcher: PyObject,
        include_directories: bool,
        last_write: u32,
        thread_count: u8,
    ) -> PyResult<pendingchanges> {
        let matcher = extract_matcher(py, pymatcher)?;
        let fs = self.filesystem(py);
        let manifest = pymanifest.get_underlying(py);
        let store = pystore.into();
        let treestate = pytreestate.get_state(py);
        let last_write = last_write.into();
        let pending = fs.borrow()
            .pending_changes(manifest, store, treestate, matcher, include_directories, last_write, thread_count)
            .map_pyerr(py)?;
        pendingchanges::create_instance(py, RefCell::new(pending))
    }
});

py_class!(class watchman |py| {
    data filesystem: RefCell<Watchman>;

    def __new__(_cls, root: PyPathBuf) -> PyResult<watchman> {
        watchman::create_instance(py, RefCell::new(Watchman::new(root.to_path_buf()).map_pyerr(py)?))
    }

    def pendingchanges(
        &self,
        pytreestate: treestate,
        last_write: u32,
        pymanifest: treemanifest,
        pystore: ImplInto<ArcReadFileContents>,
    ) -> PyResult<watchmanpendingchanges> {
        let fs = self.filesystem(py);
        let manifest = pymanifest.get_underlying(py);
        let store = pystore.into();
        let treestate = pytreestate.get_state(py);
        let last_write = last_write.into();
        let pending = Box::new(fs.borrow()
            .pending_changes(treestate, last_write, manifest, store)
            .map_pyerr(py)?);
        watchmanpendingchanges::create_instance(py, RefCell::new(pending))
    }
});

py_class!(class watchmanpendingchanges |py| {
    data inner: RefCell<Box<dyn Iterator<Item = Result<PendingChangeResult>> + Sync + Send>>;

    def __iter__(&self) -> PyResult<Self> {
        Ok(self.clone_ref(py))
    }

    def __next__(&self) -> PyResult<Option<(PyPathBuf, bool)>> {
        loop {
            match self.inner(py).borrow_mut().next() {
                Some(Ok(change)) => {
                    if let PendingChangeResult::File(change_type) = change {
                        return Ok(Some(match change_type {
                            ChangeType::Changed(path) => (path.into(), true),
                            ChangeType::Deleted(path) => (path.into(), false),
                        }));
                    }
                },
                Some(Err(_)) => {
                    // TODO: Add error handling
                    continue
                },
                None => return Ok(None),
            };
        }
    }
});

py_class!(class pendingchanges |py| {
    data inner: RefCell<PendingChanges<Arc<dyn Matcher + Sync + Send>>>;

    def __iter__(&self) -> PyResult<Self> {
        Ok(self.clone_ref(py))
    }

    def __next__(&self) -> PyResult<Option<(PyPathBuf, bool)>> {
        loop {
            match self.inner(py).borrow_mut().next() {
                Some(Ok(change)) => {
                    if let PendingChangeResult::File(change_type) = change {
                        return Ok(Some(match change_type {
                            ChangeType::Changed(path) => (path.into(), true),
                            ChangeType::Deleted(path) => (path.into(), false),
                        }));
                    }
                },
                Some(Err(_)) => {
                    // TODO: Add error handling
                    continue
                },
                None => return Ok(None),
            };
        }
    }
});
py_class!(class walker |py| {
    data inner: RefCell<Walker<Arc<dyn Matcher + Sync + Send>>>;
    data _errors: RefCell<Vec<Error>>;
    def __new__(_cls, root: PyPathBuf, pymatcher: PyObject, include_directories: bool, thread_count: u8) -> PyResult<walker> {
        let matcher = extract_matcher(py, pymatcher)?;
        let walker = Walker::new(root.to_path_buf(), matcher, include_directories, thread_count).map_pyerr(py)?;
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
    def compute(
        pymanifest: treemanifest,
        pytreestate: treestate,
        pypendingchanges: PyObject,
        pymatcher: PyObject,
    ) -> PyResult<PyObject> {
        // Convert the pending changes (a Iterable[Tuple[str, bool]]) into a Vec<ChangeType>.
        let mut pending_changes = Vec::<ChangeType>::new();
        for change in pypendingchanges.iter(py)? {
            let tuple: PyTuple = change?.cast_into(py)?;
            let file: PyString = tuple.get_item(py, 0).cast_into(py)?;
            let file = file.to_string(py)?;
            let file = RepoPathBuf::from_string(file.to_string()).map_pyerr(py)?;
            let file_exists = tuple.get_item(py, 1).cast_into::<PyBool>(py)?.is_true();
            let change = if file_exists {
                ChangeType::Changed(file)
            } else {
                ChangeType::Deleted(file)
            };
            pending_changes.push(change);
        }

        let manifest = pymanifest.get_underlying(py);
        let treestate = pytreestate.get_state(py);
        let matcher = extract_matcher(py, pymatcher)?;
        let status = workingcopy::status::compute_status(
            &*manifest.read(),
            treestate,
            pending_changes.into_iter(),
            matcher,
        ).map_pyerr(py)?;
        pystatus::to_python_status(py, &status)
    }
});
