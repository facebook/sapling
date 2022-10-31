/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::format_err;
use async_runtime::try_block_unless_interrupted as block_on;
use cpython::*;
use cpython_ext::convert::BytesLike;
use cpython_ext::convert::Serde;
use cpython_ext::ExtractInner;
use cpython_ext::PyCell;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::ResultPyErrExt;
use cpython_ext::Str;
use dag::ops::DagExportCloneData;
use dag::ops::DagImportCloneData;
use dag::ops::DagPersistent;
use dag::ops::Parents;
use dag::ops::ToIdSet;
use dag::CloneData;
use dag::Dag;
use dag::DagAlgorithm;
use dag::Vertex;
use dag::VertexListWithOptions;
use hgcommits::DagCommits;
use hgcommits::DoubleWriteCommits;
use hgcommits::GitSegmentedCommits;
use hgcommits::GraphNode;
use hgcommits::HgCommit;
use hgcommits::HgCommits;
use hgcommits::HybridCommits;
use hgcommits::MemHgCommits;
use hgcommits::RevlogCommits;
use minibytes::Bytes;
use parking_lot::RwLock;
use pyedenapi::PyClient;
use pymetalog::metalog as PyMetaLog;
use storemodel::ReadRootTreeIds;

use crate::dagalgo::dagalgo;
use crate::idmap;
use crate::Names;
use crate::Spans;

py_class!(pub class commits |py| {
    data inner: Arc<RwLock<Box<dyn DagCommits + Send + 'static>>>;

    /// Add a list of commits (node, [parent], text) in-memory.
    def addcommits(&self, commits: Vec<(PyBytes, Vec<PyBytes>, PyBytes)>) -> PyResult<PyNone> {
        let commits: Vec<HgCommit> = commits.into_iter().map(|(node, parents, raw_text)| {
            let vertex = node.data(py).to_vec().into();
            let parents = parents.into_iter().map(|p| p.data(py).to_vec().into()).collect();
            let raw_text = raw_text.data(py).to_vec().into();
            HgCommit { vertex, parents, raw_text }
        }).collect();
        let mut inner = self.inner(py).write();
        block_on(inner.add_commits(&commits)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Add a list of graph nodes (node, [parent]) in-memory.
    /// This is only supported by backends with lazy commit message support.
    def addgraphnodes(&self, commits: Vec<(PyBytes, Vec<PyBytes>)>) -> PyResult<PyNone> {
        let graph_nodes: Vec<GraphNode> = commits.into_iter().map(|(node, parents)| {
            let vertex = node.data(py).to_vec().into();
            let parents = parents.into_iter().map(|p| p.data(py).to_vec().into()).collect();
            GraphNode { vertex, parents }
        }).collect();
        let mut inner = self.inner(py).write();
        block_on(inner.add_graph_nodes(&graph_nodes)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Flush in-memory commit data and graph to disk.
    /// `masterheads` is a hint about what parts belong to the "master" group.
    def flush(&self, masterheads: Vec<PyBytes>) -> PyResult<PyNone> {
        let heads = masterheads.into_iter().map(|h| h.data(py).to_vec().into()).collect::<Vec<_>>();
        let mut inner = self.inner(py).write();
        block_on(inner.flush(&heads)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Flush in-memory commit data to disk.
    /// For the revlog backend, this also write the commit graph to disk.
    /// For the lazy commit hash backend, this also writes the commit hashes.
    def flushcommitdata(&self) -> PyResult<PyNone> {
        let mut inner = self.inner(py).write();
        block_on(inner.flush_commit_data()).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Import clone data (inside PyCell) and flush.
    def importclonedata(&self, data: PyCell) -> PyResult<PyNone> {
        let data: Box<CloneData<Vertex>> = data.take(py).ok_or_else(|| format_err!("Data is not CloneData")).map_pyerr(py)?;
        let mut inner = self.inner(py).write();
        block_on(inner.import_clone_data(*data)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Import pull data (inside PyCell) and flush.
    /// Returns (commit_count, segment_count) on success.
    def importpulldata(&self, data: PyCell, heads: Serde<VertexListWithOptions>) -> PyResult<(u64, usize)> {
        let data: Box<CloneData<Vertex>> = data.take(py).ok_or_else(|| format_err!("Data is not CloneData")).map_pyerr(py)?;
        let commits = data.flat_segments.vertex_count();
        let segments = data.flat_segments.segment_count();
        let mut inner = self.inner(py).write();
        block_on(inner.import_pull_data(*data, &heads.0)).map_pyerr(py)?;
        Ok((commits, segments))
    }

    /// Strip commits. ONLY used to make LEGACY TESTS running.
    /// Fails if called in a non-test environment.
    /// New tests should avoid depending on `strip`.
    def strip(&self, set: Names) -> PyResult<PyNone> {
        let mut inner = self.inner(py).write();
        block_on(inner.strip_commits(set.0)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Lookup the raw text of a commit by binary commit hash.
    def getcommitrawtext(&self, node: PyBytes) -> PyResult<Option<PyBytes>> {
        let vertex = node.data(py).to_vec().into();
        let inner = self.inner(py).read();
        let optional_bytes = block_on(inner.get_commit_raw_text(&vertex)).map_pyerr(py)?;
        Ok(optional_bytes.map(|bytes| PyBytes::new(py, bytes.as_ref())))
    }

    /// Lookup the raw texts by a list of binary commit hashes.
    def getcommitrawtextlist(&self, nodes: Vec<BytesLike<Vertex>>) -> PyResult<Vec<BytesLike<Bytes>>>
    {
        let vertexes: Vec<Vertex> = nodes.into_iter().map(|b| b.0).collect();
        let inner = self.inner(py).read();
        let texts = block_on(inner.get_commit_raw_text_list(&vertexes)).map_pyerr(py)?;
        Ok(texts.into_iter().map(BytesLike).collect())
    }

    /// Convert Set to IdSet. For compatibility with legacy code only.
    def torevs(&self, set: Names) -> PyResult<Spans> {
        // Attempt to use IdMap bound to `set` if possible for performance.
        let id_map = match set.0.hints().id_map() {
            Some(map) => map,
            None => self.inner(py).read().id_map_snapshot().map_pyerr(py)?,
        };
        Ok(Spans(block_on(id_map.to_id_set(&set.0)).map_pyerr(py)?))
    }

    /// Convert IdSet to Set. For compatibility with legacy code only.
    def tonodes(&self, set: Spans) -> PyResult<Names> {
        let inner = self.inner(py).read();
        Ok(Names(inner.to_set(&set.0).map_pyerr(py)?))
    }

    /// Obtain the read-only dagalgo object that supports various DAG algorithms.
    def dagalgo(&self) -> PyResult<dagalgo> {
        dagalgo::from_arc_dag(py, self.inner(py).read().dag_snapshot().map_pyerr(py)?)
    }

    /// Obtain the read-only object that can do hex prefix lookup and convert
    /// between binary commit hashes and integer Ids.
    def idmap(&self) -> PyResult<idmap::idmap> {
        idmap::idmap::from_arc_idmap(py, self.inner(py).read().id_map_snapshot().map_pyerr(py)?)
    }

    /// Name of the backend used for DAG algorithms.
    def algorithmbackend(&self) -> PyResult<Str> {
        let inner = self.inner(py).read();
        Ok(inner.algorithm_backend().to_string().into())
    }

    /// Describe the backend.
    def describebackend(&self) -> PyResult<Str> {
        let inner = self.inner(py).read();
        Ok(inner.describe_backend().into())
    }

    /// Explain internal data.
    def explaininternals(&self, out: PyObject) -> PyResult<PyNone> {
        // This function takes a 'out' parameter so it can work with pager
        // and output progressively.
        let inner = self.inner(py).read();
        let mut out = cpython_ext::wrap_pyio(out);
        inner.explain_internals(&mut out).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// checkuniversalids() -> [id]
    ///
    /// Check for missing universal ids.
    /// Returns missing ids. A valid lazy graph should return an empty list.
    /// See document in the dag crate for details.
    def checkuniversalids(&self) -> PyResult<Vec<u64>> {
        let inner = self.inner(py).read();
        let ids = block_on(inner.check_universal_ids()).map_pyerr(py)?;
        Ok(ids.into_iter().map(|i| i.0).collect())
    }

    /// checksegments() -> [str]
    ///
    /// Check for problems of segments such as cycles or wrong flags.
    /// Returns a list of human-readable messages indicating problems.
    /// A valid graph should return an empty list.
    def checksegments(&self) -> PyResult<Vec<String>> {
        let inner = self.inner(py).read();
        let problems = block_on(inner.check_segments()).map_pyerr(py)?;
        Ok(problems)
    }

    /// checkisomorphicgraph(inner, heads) -> [str]
    ///
    /// Check for problems of segments such as cycles or wrong flags.
    /// Returns a list of human-readable messages indicating problems.
    /// A valid graph should return an empty list.
    def checkisomorphicgraph(&self, other: commits, heads: Names) -> PyResult<Vec<String>> {
        let inner = self.inner(py).read();
        let other = other.inner(py).read().dag_snapshot().map_pyerr(py)?;
        let heads = heads.0;
        let problems = block_on(inner.check_isomorphic_graph(&other, heads)).map_pyerr(py)?;
        Ok(problems)
    }

    /// updatereferences(metalog)
    ///
    /// Update commit references to match metalog. Useful when metalog is not the
    /// source of truth of commit references (ex. using git references as source
    /// of truth).
    def updatereferences(&self, metalog: PyMetaLog) -> PyResult<PyNone> {
        let meta = metalog.metalog_rwlock(py);
        let mut inner = self.inner(py).write();
        inner.update_references_to_match_metalog(&meta.read()).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// migratesparsesegments(src, dst, heads=[]).
    ///
    /// Load full Dag from src directory, migrate a subset of dag to dst directory.
    ///
    /// If heads is empty, then only the master group and IdMap that are essential
    /// are migrated. If heads is not empty, specified vertexes and their ancestors
    /// are also migrated.
    ///
    /// This can be used to create a commit backend with lazy commit hashes
    /// from an existing repo.
    @staticmethod
    def migratesparsesegments(src: &PyPath, dst: &PyPath, heads: Vec<PyBytes> = Vec::new()) -> PyResult<PyNone> {
        let src = Dag::open(src.as_path()).map_pyerr(py)?;
        let mut dst = Dag::open(dst.as_path()).map_pyerr(py)?;
        let clone_data = py.allow_threads(|| block_on(src.export_clone_data())).map_pyerr(py)?;
        py.allow_threads(|| block_on(dst.import_clone_data(clone_data))).map_pyerr(py)?;

        // Also migrate specified heads and their ancestors.
        let heads: Vec<Vertex> = heads.into_iter().map(|h| h.data(py).to_vec().into()).collect::<Vec<_>>();
        let src_snapshot = src.dag_snapshot().map_pyerr(py)?;
        dst.set_remote_protocol(Arc::new(src));
        let src_dag: &dyn Parents = &src_snapshot;
        let heads = VertexListWithOptions::from(heads);
        py.allow_threads(|| block_on(dst.add_heads_and_flush(src_dag, &heads))).map_pyerr(py)?;

        Ok(PyNone)
    }

    /// Construct `commits` from a revlog (`00changelog.i` and `00changelog.d`).
    @staticmethod
    def openrevlog(dir: &PyPath) -> PyResult<Self> {
        let inner = RevlogCommits::new(dir.as_path()).map_pyerr(py)?;
        Self::from_commits(py, inner)
    }

    /// Construct `commits` from a segmented changelog + hgcommits directory.
    @staticmethod
    def opensegments(segmentsdir: &PyPath, commitsdir: &PyPath) -> PyResult<Self> {
        let inner = HgCommits::new(segmentsdir.as_path(), commitsdir.as_path()).map_pyerr(py)?;
        Self::from_commits(py, inner)
    }

    /// Migrate from revlog to segmented changelog (full IdMap).
    ///
    /// This does not migrate commit texts and therefore only useful for
    /// doublewrite backend.
    @staticmethod
    def migraterevlogtosegments(revlogdir: &PyPath, segmentsdir: &PyPath, commitsdir: &PyPath, master: Names) -> PyResult<PyNone> {
        let revlog = RevlogCommits::new(revlogdir.as_path()).map_pyerr(py)?;
        let mut segments = HgCommits::new(segmentsdir.as_path(), commitsdir.as_path()).map_pyerr(py)?;
        py.allow_threads(|| block_on(segments.import_dag(revlog, master.0))).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Construct "double write" `commits` from both revlog and segmented
    /// changelog.
    @staticmethod
    def opendoublewrite(revlogdir: &PyPath, segmentsdir: &PyPath, commitsdir: &PyPath) -> PyResult<Self> {
        let inner = DoubleWriteCommits::new(revlogdir.as_path(), segmentsdir.as_path(), commitsdir.as_path()).map_pyerr(py)?;
        Self::from_commits(py, inner)
    }

    /// Construct `commits` from a revlog + segmented changelog + hgcommits + edenapi hybrid.
    ///
    /// This is similar to doublewrite backend, except that commit text fallback is edenapi,
    /// not revlog, despite the revlog might have the data.
    ///
    /// If lazyhash is True, enable lazy commit hashes or EdenAPI.
    ///
    /// If lazyhashdir is set, enable lazy commit hashes backed by the given segments dir
    /// (for testing).
    @staticmethod
    def openhybrid(
        revlogdir: Option<&PyPath>, segmentsdir: &PyPath, commitsdir: &PyPath, edenapi: PyClient,
        lazyhash: bool = false, lazyhashdir: Option<&PyPath> = None
    ) -> PyResult<Self> {
        let client = edenapi.extract_inner(py);
        let mut inner = HybridCommits::new(
            revlogdir.map(|d| d.as_path()),
            segmentsdir.as_path(),
            commitsdir.as_path(),
            client,
        ).map_pyerr(py)?;
        if let Some(dir) = lazyhashdir {
            inner.enable_lazy_commit_hashes_from_local_segments( dir.as_path()).map_pyerr(py)?;
        } else if lazyhash {
            inner.enable_lazy_commit_hashes();
        }
        Self::from_commits(py, inner)
    }

    /// Construct "git segmented" `commits` from a git repo and segmented
    /// changelog.
    @staticmethod
    def opengitsegments(gitdir: &PyPath, segmentsdir: &PyPath, metalog: PyMetaLog) -> PyResult<Self> {
        let inner = py.allow_threads(|| GitSegmentedCommits::new(gitdir.as_path(), segmentsdir.as_path())).map_pyerr(py)?;
        let meta = metalog.metalog_rwlock(py);
        inner.git_references_to_metalog(&mut meta.write()).map_pyerr(py)?;
        Self::from_commits(py, inner)
    }

    /// Construct a private, empty `commits` object backed by the memory.
    /// `flush` does nothing for this type of object.
    @staticmethod
    def openmemory() -> PyResult<Self> {
        let inner = MemHgCommits::new().map_pyerr(py)?;
        Self::from_commits(py, inner)
    }
});

impl commits {
    /// Create a `commits` Python object from a Rust struct.
    pub fn from_commits(py: Python, commits: impl DagCommits + Send + 'static) -> PyResult<Self> {
        Self::create_instance(py, Arc::new(RwLock::new(Box::new(commits))))
    }

    pub(crate) fn to_read_root_tree_nodes(
        &self,
        py: Python,
    ) -> Arc<dyn ReadRootTreeIds + Send + Sync> {
        let inner = self.inner(py).read();
        inner.to_dyn_read_root_tree_ids()
    }

    pub fn get_inner(&self, py: Python) -> Arc<RwLock<Box<dyn DagCommits + Send + 'static>>> {
        self.inner(py).clone()
    }
}
