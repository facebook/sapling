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
use cpython_ext::convert::Serde;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::PyPathBuf;
use io::IO;
use parking_lot::RwLock;
use pathmatcher::Matcher;
use pyconfigloader::config;
#[cfg(feature = "eden")]
use pyedenclient::feature_eden::EdenFsClient as PyEdenClient;
use pypathmatcher::extract_matcher;
use pypathmatcher::extract_option_matcher;
use pypathmatcher::treematcher;
use pytreestate::treestate;
use rsworkingcopy::walker::WalkError;
use rsworkingcopy::walker::Walker;
use rsworkingcopy::workingcopy::WorkingCopy;
use types::HgId;

#[cfg(not(feature = "eden"))]
py_class!(pub class PyEdenClient |py| {
    data inner: Arc<rsworkingcopy::workingcopy::EdenFsClient>;
});

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "workingcopy"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<walker>(py)?;
    m.add_class::<workingcopy>(py)?;

    m.add(
        py,
        "parsegitsubmodules",
        py_fn!(py, parse_git_submodules(data: &PyBytes)),
    )?;

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
    ) -> PyResult<walker> {
        let matcher = extract_matcher(py, pymatcher)?;
        let walker = Walker::new(
            root.to_path_buf(),
            dot_dir.clone(),
            vec![dot_dir.into()],
            matcher,
            include_directories,
        ).map_pyerr(py)?;
        walker::create_instance(py, RefCell::new(walker), RefCell::new(Vec::new()))
    }

    def __iter__(&self) -> PyResult<Self> {
        Ok(self.clone_ref(py))
    }

    def __next__(&self) -> PyResult<Option<PyPathBuf>> {
        loop {
            let inner = &mut *self.inner(py).borrow_mut();
            match py.allow_threads(|| inner.next()) {
                Some(Ok(path)) => return Ok(Some(PyPathBuf::from(path.as_ref()))),
                Some(Err(e)) => self._errors(py).borrow_mut().push(e),
                None => return Ok(None),
            };
        }
    }

    def errors(&self) -> PyResult<Vec<(String, String)>> {
        Ok(self._errors(py).borrow().iter().map(|e| match e.downcast_ref::<WalkError>() {
            Some(e) => (e.filename(), e.message()),
            None => ("unknown".to_string(), e.to_string()),
        }).collect::<Vec<(String, String)>>())
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
        include_ignored: bool,
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
                wc.status(matcher, last_write, include_ignored, &config, &io)
            }).map_pyerr(py)?
        )
    }

    // Fetch list of (treematcher, rule_details) to be unioned together.
    // rule_details drive the sparse "explain" functionality.
    def sparsematchers(
        &self,
        nodes: Serde<Vec<HgId>>,
        raw_config: Option<(String, String)>,
        debug_version: Option<String>,
        no_catch_all: bool,
    ) -> PyResult<Vec<(treematcher, Vec<String>)>> {
        let wc = self.inner(py).read();

        let mut prof = match raw_config {
            Some((contents, source)) => sparse::Root::from_bytes(contents.into_bytes(), source).map_pyerr(py)?,
            None => {
                let repo_sparse_path = wc.dot_hg_path().join("sparse");
                match fs_err::read(&repo_sparse_path) {
                    Ok(contents) => sparse::Root::from_bytes(contents, repo_sparse_path.display().to_string()).map_pyerr(py)?,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                    Err(e) => return Err(e).map_pyerr(py),
                }
            },
        };

        if debug_version.is_some() {
            prof.set_version_override(debug_version);
        }

        prof.set_skip_catch_all(no_catch_all);

        let overrides = rsworkingcopy::sparse::disk_overrides(wc.dot_hg_path()).map_pyerr(py)?;

        let mut all_tree_matchers = Vec::new();
        for node in &*nodes {
            let mf = wc.tree_resolver().get(node).map_pyerr(py)?;
            let matcher = rsworkingcopy::sparse::build_matcher(&prof, mf.read().clone(), wc.filestore(), &overrides).map_pyerr(py)?.0;
            let tree_matchers = matcher.into_matchers();
            if tree_matchers.is_empty() {
                return Ok(Vec::new());
            }
            all_tree_matchers.extend(tree_matchers.into_iter());
        }
        all_tree_matchers
            .into_iter()
            .map(|(tm, origins)| Ok((treematcher::create_instance(py, Arc::new(tm))?, origins)))
            .collect::<PyResult<Vec<_>>>()
    }

    def edenclient(&self) -> PyResult<PyEdenClient> {
        let wc = self.inner(py).read();
        PyEdenClient::create_instance(py, wc.eden_client().map_pyerr(py)?)
    }
});

fn parse_git_submodules(py: Python, data: &PyBytes) -> PyResult<Vec<(String, String, String)>> {
    Ok(rsworkingcopy::git::parse_submodules(data.data(py))
        .map_pyerr(py)?
        .into_iter()
        .map(|sm| (sm.name, sm.url, sm.path))
        .collect())
}
