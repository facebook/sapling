/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::collections::HashMap;
use std::sync::Arc;

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
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use dag::DagAlgorithm;
use dag::Vertex;
use pypathmatcher::extract_matcher;
use storemodel::types::RepoPathBuf;
use storemodel::FileStore;
use storemodel::ReadRootTreeIds;
use storemodel::SerializationFormat;
use storemodel::TreeStore;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "copytrace"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<dagcopytrace>(py)?;
    m.add(
        py,
        "is_content_similar",
        py_fn!(py, is_content_similar(
            a: PyBytes,
            b: PyBytes,
            config: ImplInto<Arc<dyn Config + Send + Sync>>,
        )),
    )?;
    m.add(
        py,
        "content_similarity",
        py_fn!(py, content_similarity(
            a: PyBytes,
            b: PyBytes,
            config: ImplInto<Arc<dyn Config + Send + Sync>>,
            threshold: Option<f32> = None,
        )),
    )?;
    Ok(m)
}

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
            SerializationFormat::Hg => Arc::new(
                MetadataRenameFinder::new(file_reader.into(), config.clone()).map_pyerr(py)?),
            SerializationFormat::Git => Arc::new(
                ContentSimilarityRenameFinder::new(file_reader.into(), config.clone()).map_pyerr(py)?),
        };
        let dag = dag.into();

        let copytrace = DagCopyTrace::new(
            root_tree_reader,
            tree_store,
            rename_finder,
            dag,
            config,
        ).map_pyerr(py)?;
        Self::create_instance(py, Arc::new(copytrace))
    }

    /// trace_renames(src: node, dst: node, src_paths: list[str]) -> dict[dst_path, src_path]
    ///
    /// Find the renamed-to paths of `src_paths` from `src` commit to `dst` commit.
    def trace_renames(
        &self,
        src: PyBytes,
        dst: PyBytes,
        src_paths: Vec<PyPathBuf>,
    ) -> PyResult<HashMap<String, String>> {
        let src = Vertex::copy_from(src.data(py));
        let dst = Vertex::copy_from(dst.data(py));
        let src_paths = src_paths
            .into_iter()
            .map(|p| p.to_repo_path_buf().map_pyerr(py))
            .collect::<PyResult<Vec<_>>>()?;
        let inner = self.inner(py).clone();
        let result = py.allow_threads(|| block_on(trace_renames(inner, src, dst, src_paths))).map_pyerr(py)?;
        Ok(result)
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

    /// path_copies(src: node, dst: node, matcher: Optional[Matcher] = None) -> Dict[str, str]
    ///
    /// find {dst: src} copy mapping for directed compare.
    def path_copies(
        &self,
        src: PyBytes,
        dst: PyBytes,
        matcher: Option<PyObject> = None,
    ) -> PyResult<HashMap<String, String>> {
        let src = Vertex::copy_from(src.data(py));
        let dst = Vertex::copy_from(dst.data(py));
        let matcher = match matcher {
            Some(obj) => Some(extract_matcher(py, obj)?.0),
            None => None,
        };

        let inner = self.inner(py).clone();
        let copies = py.allow_threads(|| block_on(inner.path_copies(src, dst, matcher))).map_pyerr(py)?;

        let copies = copies.into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Ok(copies)
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

fn content_similarity(
    py: Python,
    a: PyBytes,
    b: PyBytes,
    config: ImplInto<Arc<dyn Config + Send + Sync>>,
    threshold: Option<f32>,
) -> PyResult<(bool, f32)> {
    let a = a.data(py);
    let b = b.data(py);
    let config = config.into();
    py.allow_threads(|| copytrace::content_similarity(a, b, &config, threshold))
        .map_pyerr(py)
}

async fn trace_renames(
    dagcopytrace: Arc<DagCopyTrace>,
    src: Vertex,
    dst: Vertex,
    src_paths: Vec<RepoPathBuf>,
) -> Result<HashMap<String, String>, std::io::Error> {
    let mut renames = HashMap::new();
    for src_path in src_paths {
        let dst_path = dagcopytrace
            .trace_rename(src.clone(), dst.clone(), src_path.clone())
            .await;
        if let Ok(TraceResult::Renamed(dst_path)) = dst_path {
            renames.insert(dst_path.to_string(), src_path.to_string());
        }
    }
    Ok(renames)
}
