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
use configmodel::Config;
use copytrace::ContentSimilarityRenameFinder;
use copytrace::CopyTrace;
use copytrace::DagCopyTrace;
use copytrace::MetadataRenameFinder;
use copytrace::RenameFinder;
use copytrace::TraceResult;
use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::convert::Serde;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use dag::DagAlgorithm;
use dag::Vertex;
use parking_lot::Mutex;
use storemodel::FileStore;
use storemodel::ReadRootTreeIds;
use storemodel::SerializationFormat;
use storemodel::TreeStore;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "copytrace"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<gitcopytrace>(py)?;
    m.add_class::<dagcopytrace>(py)?;
    m.add(
        py,
        "is_content_similar",
        py_fn!(py, is_content_similar(
    a: PyBytes,
    b: PyBytes,
    config: ImplInto<Arc<dyn Config + Send + Sync>>)),
    )?;
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
        tree_store: ImplInto<Arc<dyn TreeStore>>,
        file_reader: ImplInto<Arc<dyn FileStore>>,
        dag: ImplInto<Arc<dyn DagAlgorithm + Send + Sync>>,
        config: ImplInto<Arc<dyn Config + Send + Sync>>,
    ) -> PyResult<Self> {
        let root_tree_reader = root_tree_reader.into();
        let tree_store = tree_store.into();
        let config = config.into();
        let rename_finder: Arc<dyn RenameFinder + Send + Sync> = match tree_store.format() {
            SerializationFormat::Hg => Arc::new(MetadataRenameFinder::new(file_reader.into(), config).map_pyerr(py)?),
            SerializationFormat::Git => Arc::new(ContentSimilarityRenameFinder::new(file_reader.into(), config).map_pyerr(py)?),
        };
        let dag = dag.into();

        let copytrace = DagCopyTrace::new(
            root_tree_reader,
            tree_store,
            rename_finder,
            dag,
        ).map_pyerr(py)?;
        Self::create_instance(py, Arc::new(copytrace))
    }

    /// trace_rename(src: node, dst: node, src_path: str) -> Optional[dst_path]
    ///
    /// Find the renamed-to path of `src_path` from `src` commit to `dst` commit.
    /// If not found, return None.
    def trace_rename(
        &self,
        src: PyBytes,
        dst: PyBytes,
        src_path: PyPathBuf,
    ) -> PyResult<Option<String>> {
        let src = Vertex::copy_from(src.data(py));
        let dst = Vertex::copy_from(dst.data(py));
        let src_path = src_path.to_repo_path_buf().map_pyerr(py)?;
        let inner = self.inner(py).clone();
        let trace_result = py.allow_threads(|| block_on(inner.trace_rename(src, dst, src_path))).map_pyerr(py)?;
        match trace_result {
            TraceResult::Renamed(path) => Ok(Some(path.to_string())),
            _ => Ok(None),
        }
    }

    /// trace_rename(src: node, dst: node, src_path: str) -> TraceResult
    ///
    /// Find the renamed-to path of `src_path` from `src` commit to `dst` commit.
    /// If not found, return the commit that added/deleted the given source file.
    def trace_rename_ex(
        &self,
        src: PyBytes,
        dst: PyBytes,
        src_path: PyPathBuf,
    ) -> PyResult<Serde<TraceResult>> {
        let src = Vertex::copy_from(src.data(py));
        let dst = Vertex::copy_from(dst.data(py));
        let src_path = src_path.to_repo_path_buf().map_pyerr(py)?;
        let inner = self.inner(py).clone();
        let trace_result = py.allow_threads(|| block_on(inner.trace_rename(src, dst, src_path))).map_pyerr(py)?;
        Ok(Serde(trace_result))
    }
});

fn is_content_similar(
    py: Python,
    a: PyBytes,
    b: PyBytes,
    config: ImplInto<Arc<dyn Config + Send + Sync>>,
) -> PyResult<bool> {
    let a = a.data(py);
    let b = b.data(py);
    let config = config.into();
    py.allow_threads(|| copytrace::is_content_similar(a, b, &config))
        .map_pyerr(py)
}
