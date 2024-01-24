/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

extern crate workingcopy as rsworkingcopy;

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Error;
use cpython::*;
use cpython_ext::convert::Serde;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::ExtractInnerRef;
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
use repostate::command_state::Operation;
use rsworkingcopy::walker::WalkError;
use rsworkingcopy::walker::Walker;
use rsworkingcopy::workingcopy::WorkingCopy;
use termlogger::TermLogger;
use types::HgId;

#[cfg(not(feature = "eden"))]
py_class!(pub class PyEdenClient |py| {
    data inner: Arc<rsworkingcopy::workingcopy::EdenFsClient>;
});

mod impl_into;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "workingcopy"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<mergestate>(py)?;
    m.add_class::<walker>(py)?;
    m.add_class::<workingcopy>(py)?;

    m.add(
        py,
        "parsegitsubmodules",
        py_fn!(py, parse_git_submodules(data: &PyBytes)),
    )?;

    impl_into::register(py);

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
        include_ignored: bool,
        config: &config,
    ) -> PyResult<PyObject> {
        let wc = self.inner(py).write();
        let matcher = extract_option_matcher(py, pymatcher)?;
        let io = IO::main().map_pyerr(py)?;
        let config = config.get_cfg(py);
        pystatus::to_python_status(py,
            &py.allow_threads(|| {
                wc.status(matcher, include_ignored, &config, &TermLogger::new(&io))
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
            let matcher = rsworkingcopy::sparse::build_matcher(&prof, &mf, wc.filestore(), &overrides).map_pyerr(py)?.0;
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

    def mergestate(&self) -> PyResult<mergestate> {
        match self.inner(py).read().read_merge_state() {
            Ok(None) => mergestate::create_instance(py, RefCell::new(repostate::MergeState::default())),
            Ok(Some(ms)) => mergestate::create_instance(py, RefCell::new(ms)),
            Err(err) => match err.downcast::<repostate::UnsupportedMergeRecords>() {
                Ok(bad) => {
                    let bad_types: Vec<String> = bad.0.unsupported_records()
                        .iter()
                        .filter_map(|(t, _)| if t.len() == 1 && t.as_bytes()[0].is_ascii_lowercase() {
                            None
                        } else {
                            Some(t.to_string())
                        })
                        .collect();
                    Err(PyErr::from_instance(py, py.import("sapling.error")?
                                                    .get(py, "UnsupportedMergeRecords")?
                                                    .call(py, (bad_types.into_py_object(py),), None)?))
                }
                Err(err) => Err(err).map_pyerr(py),
            },

        }
    }

    def writemergestate(&self, ms: mergestate) -> PyResult<PyNone> {
        let ms = ms.extract_inner_ref(py);
        self.inner(py).read().lock().map_pyerr(py)?.write_merge_state(&*ms.borrow()).map_pyerr(py)?;
        Ok(PyNone)
    }

    // Return repo's unfinished command state (e.g. resolving conflicts for
    // "rebase"), if any. Return value is (<state description>, <hint for user>).
    // `op` optionally specifies what operation you are
    // performing (some unfinished command states allow certain operations such
    // as "commit").
    def commandstate(&self, op: Option<Serde<Operation>> = None) -> PyResult<Option<(String, String)>> {
        let wc = self.inner(py).read();
        let op = op.map_or(Operation::Other, |op| *op);
        match repostate::command_state::try_operation(wc.dot_hg_path(), op) {
            Ok(()) => Ok(None),
            Err(err) => {
                if let Some(conflict) = err.downcast_ref::<repostate::command_state::Conflict>() {
                    return Ok(Some((conflict.description().to_string(), conflict.hint())));
                }
                Err(err).map_pyerr(py)
            },
        }
    }
});

py_class!(pub class mergestate |py| {
    data ms: RefCell<repostate::MergeState>;

    def __new__(
        _cls,
        local: Option<PyBytes>,
        other: Option<PyBytes>,
        labels: Option<Vec<String>>,
    ) -> PyResult<Self> {
        Self::create_instance(py, RefCell::new(repostate::MergeState::new(
            local.map(|b| HgId::from_slice(b.data(py))).transpose().map_pyerr(py)?,
            other.map(|b| HgId::from_slice(b.data(py))).transpose().map_pyerr(py)?,
            labels.unwrap_or_default(),
        )))
    }

    def local(&self) -> PyResult<Option<PyBytes>> {
        Ok(self.ms(py).borrow().local().map(|l| PyBytes::new(py, l.as_ref())))
    }

    def other(&self) -> PyResult<Option<PyBytes>> {
        Ok(self.ms(py).borrow().other().map(|l| PyBytes::new(py, l.as_ref())))
    }

    def mergedriver(&self) -> PyResult<Option<(String, String)>> {
        Ok(self.ms(py).borrow().merge_driver().map(|(md, mds)| (md.to_string(), mds.to_py_string().to_string())))
    }

    def setmergedriver(&self, md: Option<(String, String)>) -> PyResult<PyNone> {
        self.ms(py).borrow_mut().set_merge_driver(
            md.map(|(md, mds)| (md, repostate::MergeDriverState::from_py_string(&mds))),
        );
        Ok(PyNone)
    }

    def labels(&self) -> PyResult<Vec<String>> {
        Ok(self.ms(py).borrow().labels().to_vec())
    }

    def insert(&self, path: PyPathBuf, data: Vec<String>) -> PyResult<PyNone> {
        let mut ms = self.ms(py).borrow_mut();
        ms.insert(path.to_repo_path_buf().map_pyerr(py)?, data).map_pyerr(py)?;
        Ok(PyNone)
    }

    def get(&self, path: PyPathBuf) -> PyResult<Option<Vec<String>>> {
        Ok(self.ms(py).borrow().files().get(path.to_repo_path().map_pyerr(py)?).map(|f| f.data().clone()))
    }

    def remove(&self, path: PyPathBuf) -> PyResult<PyNone> {
        self.ms(py).borrow_mut().remove(path.to_repo_path().map_pyerr(py)?);
        Ok(PyNone)
    }

    def setstate(&self, path: PyPathBuf, state: String) -> PyResult<PyNone> {
        self.ms(py).borrow_mut().set_state(path.to_repo_path().map_pyerr(py)?, state).map_pyerr(py)?;
        Ok(PyNone)
    }

    def files(&self, states: Option<Vec<String>> = None) -> PyResult<Vec<PyPathBuf>> {
        let filter: HashSet<_> = states.unwrap_or_default().into_iter().collect();
        Ok(self.ms(py)
           .borrow()
           .files()
           .iter()
           .filter_map(|(p,f)| if filter.is_empty() || filter.contains(&f.data()[0]) { Some(p) } else { None })
           .cloned()
           .map(Into::into)
           .collect())
    }

    def contains(&self, path: PyPathBuf) -> PyResult<bool> {
        Ok(self.ms(py).borrow().files().contains_key(path.to_repo_path().map_pyerr(py)?))
    }

    def extras(&self, path: PyPathBuf) -> PyResult<HashMap<String, String>> {
        Ok(self.ms(py).borrow().files().get(path.to_repo_path().map_pyerr(py)?).map(|f| f.extras().clone()).unwrap_or_default())
    }

    def setextra(&self, path: PyPathBuf, key: String, value: String) -> PyResult<PyNone> {
        self.ms(py).borrow_mut().set_extra(path.to_repo_path().map_pyerr(py)?, key, value).map_pyerr(py)?;
        Ok(PyNone)
    }

    def isempty(&self) -> PyResult<bool> {
        Ok(self.ms(py).borrow().files().is_empty())
    }
});

impl ExtractInnerRef for mergestate {
    type Inner = RefCell<repostate::MergeState>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.ms(py)
    }
}

impl workingcopy {
    pub fn get_wc(&self, py: Python) -> Arc<RwLock<WorkingCopy>> {
        self.inner(py).clone()
    }
}

fn parse_git_submodules(py: Python, data: &PyBytes) -> PyResult<Vec<(String, String, String)>> {
    Ok(rsworkingcopy::git::parse_submodules(data.data(py))
        .map_pyerr(py)?
        .into_iter()
        .map(|sm| (sm.name, sm.url, sm.path))
        .collect())
}
