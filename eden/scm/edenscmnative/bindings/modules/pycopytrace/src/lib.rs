/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::collections::HashMap;
use std::sync::Arc;

use ::copytrace::GitCopyTrace;
use ::types::HgId;
use async_runtime::try_block_unless_interrupted as block_on;
use copytrace::CopyTrace;
use copytrace::DagCopyTrace;
use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::convert::Serde;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use dag::DagAlgorithm;
use dag::Vertex;
use parking_lot::Mutex;
use pymanifest::treemanifest;
use storemodel::ReadFileContents;
use storemodel::ReadRootTreeIds;
use storemodel::TreeStore;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "copytrace"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<gitcopytrace>(py)?;
    m.add_class::<dagcopytrace>(py)?;
    Ok(m)
}

py_class!(pub class gitcopytrace |py| {
    data inner: Arc<Mutex<GitCopyTrace>>;

    def __new__(_cls, gitdir: &PyPath) -> PyResult<Self> {
        let copytrace = GitCopyTrace::open(gitdir.as_path()).map_pyerr(py)?;
        Self::create_instance(py, Arc::new(Mutex::new(copytrace)))
    }

    /// Find copies between old and new commits, the result is a {newpath: oldpath} map.
    def findcopies(
        &self, oldnode: Serde<HgId>, newnode: Serde<HgId>
    ) -> PyResult<HashMap<String, String>> {
        let copytrace = self.inner(py).lock();
        let copies = copytrace.find_copies(oldnode.0, newnode.0).map_pyerr(py)?;
        Ok(copies)
    }
});

py_class!(pub class dagcopytrace |py| {
    data inner: Arc<DagCopyTrace>;

    def __new__(
        _cls,
        root_tree_reader: ImplInto<Arc<dyn ReadRootTreeIds + Send + Sync>>,
        tree_store: ImplInto<Arc<dyn TreeStore + Send + Sync>>,
        file_reader: ImplInto<Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>>,
        dag: ImplInto<Arc<dyn DagAlgorithm + Send + Sync>>,
    ) -> PyResult<Self> {
        let root_tree_reader = root_tree_reader.into();
        let tree_store = tree_store.into();
        let file_reader = file_reader.into();
        let dag = dag.into();
        let copytrace = DagCopyTrace::new(
            root_tree_reader,
            tree_store,
            file_reader,
            dag,
        ).map_pyerr(py)?;
        Self::create_instance(py, Arc::new(copytrace))
    }

    /// trace_rename(src: node, dst: node, src_path: str) -> Optional[str].
    /// Find the renamed path in `dst` that is from the `src_path` in `src` commit.
    def trace_rename(
        &self,
        src: PyBytes,
        dst: PyBytes,
        src_path: PyPathBuf,
    ) -> PyResult<Option<String>> {
        let src = Vertex::copy_from(src.data(py));
        let dst = Vertex::copy_from(dst.data(py));
        let src_path = src_path.to_repo_path_buf().map_pyerr(py)?;
        let path = block_on(self.inner(py).trace_rename(src, dst, src_path)).map_pyerr(py)?;
        Ok(path.map(|v| v.to_string()))
    }


    /// Find renames between old and new commits, the result is a {newpath: oldpath} map.
    def find_renames(
        &self,
        old_tree: &treemanifest,
        new_tree: &treemanifest,
    ) -> PyResult<HashMap<String, String>> {
        let old_tree = old_tree.get_underlying(py);
        let new_tree = new_tree.get_underlying(py);
        let map = block_on(self.inner(py).find_renames(&old_tree.read(), &new_tree.read())).map_pyerr(py)?;
        let map = map
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<HashMap<_, _>>();
        Ok(map)
    }
});
