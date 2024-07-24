/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::sync::Arc;
use std::sync::Mutex;

use ::pathhistory::PathHistory;
use async_runtime::try_block_unless_interrupted as block_on;
use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use dag::Set;
use dag::Vertex;
use storemodel::ReadRootTreeIds;
use storemodel::TreeStore;
use types::RepoPathBuf;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "pathhistory"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<pathhistory>(py)?;
    m.add(
        py,
        "lastcreation",
        py_fn!(py, lastcreation(
            set: ImplInto<Set>,
            path: PyPathBuf,
            roottreereader: ImplInto<Arc<dyn ReadRootTreeIds + Send + Sync>>,
            treestore: ImplInto<Arc<dyn TreeStore>>,
        )),
    )?;
    Ok(m)
}

py_class!(class pathhistory |py| {
    data inner: Arc<Mutex<PathHistory>>;

    def __new__(
        _cls,
        set: ImplInto<Set>,
        paths: Vec<PyPathBuf>,
        roottreereader: ImplInto<Arc<dyn ReadRootTreeIds + Send + Sync>>,
        treestore: ImplInto<Arc<dyn TreeStore>>,
    ) -> PyResult<Self> {
        let set = set.into();
        let paths: Vec<RepoPathBuf> = paths.into_iter().map(|p| p.to_repo_path_buf()).collect::<Result<Vec<_>, _>>().map_pyerr(py)?;
        let root_tree_reader = roottreereader.into();
        let tree_store = treestore.into();
        let history = py.allow_threads(|| block_on(PathHistory::new_content_tracer(set, paths, root_tree_reader, tree_store))).map_pyerr(py)?;
        Self::create_instance(py, Arc::new(Mutex::new(history)))
    }

    def __next__(&self) -> PyResult<Option<PyBytes>> {
        let inner: Arc<_> = self.inner(py).clone();
        let next: Option<Vertex> = py.allow_threads(|| {
            let mut inner = inner.lock().unwrap();
            block_on(inner.next())
        }).map_pyerr(py)?;
        Ok(next.map(|v| PyBytes::new(py, v.as_ref())))
    }

    def __iter__(&self) -> PyResult<Self> {
        Ok(self.clone_ref(py))
    }
});

fn lastcreation(
    py: Python,
    set: ImplInto<Set>,
    path: PyPathBuf,
    roottreereader: ImplInto<Arc<dyn ReadRootTreeIds + Send + Sync>>,
    treestore: ImplInto<Arc<dyn TreeStore>>,
) -> PyResult<Option<PyBytes>> {
    let set = set.into();
    let root_tree_reader = roottreereader.into();
    let tree_store = treestore.into();
    let got = py
        .allow_threads(|| {
            block_on(async {
                let mut history = PathHistory::new_existence_tracer(
                    set,
                    path.to_repo_path_buf()?,
                    root_tree_reader,
                    tree_store,
                )
                .await?;
                history.next().await
            })
        })
        .map_pyerr(py)?;

    Ok(got.map(|n| PyBytes::new(py, n.as_ref())))
}
