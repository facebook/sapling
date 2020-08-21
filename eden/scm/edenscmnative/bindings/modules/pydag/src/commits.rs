/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::dagalgo::dagalgo;
use crate::idmap;
use crate::Names;
use crate::Spans;
use anyhow::Result;
use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::ResultPyErrExt;
use cpython_ext::Str;
use dag::ops::IdConvert;
use dag::ops::PrefixLookup;
use dag::ops::ToIdSet;
use dag::ops::ToSet;
use dag::DagAlgorithm;
use dag::Group;
use dag::Id;
use dag::Set;
use dag::Vertex;
use hgcommits::AppendCommits;
use hgcommits::DescribeBackend;
use hgcommits::DoubleWriteCommits;
use hgcommits::HgCommit;
use hgcommits::HgCommits;
use hgcommits::MemHgCommits;
use hgcommits::ReadCommitText;
use hgcommits::RevlogCommits;
use hgcommits::StripCommits;
use std::cell::RefCell;
use std::sync::Arc;

/// A combination of other traits: commit read/write + DAG algorithms.
pub trait Commits:
    ReadCommitText
    + StripCommits
    + AppendCommits
    + DescribeBackend
    + DagAlgorithm
    + IdConvert
    + PrefixLookup
    + ToIdSet
    + ToSet
{
}

impl Commits for HgCommits {}
impl Commits for MemHgCommits {}
impl Commits for RevlogCommits {}
impl Commits for DoubleWriteCommits {}

py_class!(pub class commits |py| {
    data inner: RefCell<Box<dyn Commits + Send + 'static>>;

    /// Add a list of commits (node, [parent], text) in-memory.
    def addcommits(&self, commits: Vec<(PyBytes, Vec<PyBytes>, PyBytes)>) -> PyResult<PyNone> {
        let commits: Vec<HgCommit> = commits.into_iter().map(|(node, parents, raw_text)| {
            let vertex = node.data(py).to_vec().into();
            let parents = parents.into_iter().map(|p| p.data(py).to_vec().into()).collect();
            let raw_text = raw_text.data(py).to_vec().into();
            HgCommit { vertex, parents, raw_text }
        }).collect();
        let mut inner = self.inner(py).borrow_mut();
        inner.add_commits(&commits).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Flush in-memory commits to disk.
    /// `masterheads` is a hint about what parts belong to the "master" group.
    def flush(&self, masterheads: Vec<PyBytes>) -> PyResult<PyNone> {
        let heads = masterheads.into_iter().map(|h| h.data(py).to_vec().into()).collect::<Vec<_>>();
        let mut inner = self.inner(py).borrow_mut();
        inner.flush(&heads).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Strip commits. ONLY used to make LEGACY TESTS running.
    /// Fails if called in a non-test environment.
    /// New tests should avoid depending on `strip`.
    def strip(&self, set: Names) -> PyResult<PyNone> {
        let mut inner = self.inner(py).borrow_mut();
        inner.strip_commits(set.0).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Lookup the raw text of a commit by binary commit hash.
    def getcommitrawtext(&self, node: PyBytes) -> PyResult<Option<PyBytes>> {
        let vertex = node.data(py).to_vec().into();
        let inner = self.inner(py).borrow();
        let optional_bytes = inner.get_commit_raw_text(&vertex).map_pyerr(py)?;
        Ok(optional_bytes.map(|bytes| PyBytes::new(py, bytes.as_ref())))
    }

    /// Convert Set to IdSet. For compatibility with legacy code only.
    def torevs(&self, set: Names) -> PyResult<Spans> {
        let inner = self.inner(py).borrow();
        Ok(Spans(inner.to_id_set(&set.0).map_pyerr(py)?))
    }

    /// Convert IdSet to Set. For compatibility with legacy code only.
    def tonodes(&self, set: Spans) -> PyResult<Names> {
        let inner = self.inner(py).borrow();
        Ok(Names(inner.to_set(&set.0).map_pyerr(py)?))
    }

    /// Obtain the read-only dagalgo object that supports various DAG algorithms.
    def dagalgo(&self) -> PyResult<dagalgo> {
        dagalgo::from_dag(py, self.clone_ref(py))
    }

    /// Obtain the read-only object that can do hex prefix lookup and convert
    /// between binary commit hashes and integer Ids.
    def idmap(&self) -> PyResult<idmap::idmap> {
        idmap::idmap::from_idmap(py, self.clone_ref(py))
    }

    /// Name of the backend used for DAG algorithms.
    def algorithmbackend(&self) -> PyResult<Str> {
        let inner = self.inner(py).borrow();
        Ok(inner.algorithm_backend().to_string().into())
    }

    /// Describe the backend.
    def describebackend(&self) -> PyResult<Str> {
        let inner = self.inner(py).borrow();
        Ok(inner.describe_backend().into())
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
        py.allow_threads(|| segments.import_dag(revlog, master.0)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Construct "double write" `commits` from both revlog and segmented
    /// changelog.
    @staticmethod
    def opendoublewrite(revlogdir: &PyPath, segmentsdir: &PyPath, commitsdir: &PyPath) -> PyResult<Self> {
        let inner = DoubleWriteCommits::new(revlogdir.as_path(), segmentsdir.as_path(), commitsdir.as_path()).map_pyerr(py)?;
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
    pub fn from_commits(py: Python, commits: impl Commits + Send + 'static) -> PyResult<Self> {
        Self::create_instance(py, RefCell::new(Box::new(commits)))
    }
}

// Delegate trait implementations to `inner`.

impl DagAlgorithm for commits {
    fn sort(&self, set: &Set) -> dag::Result<Set> {
        // commits are used by other Python objects: the other Python objects hold the GIL.
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().sort(set)
    }

    fn parent_names(&self, name: Vertex) -> dag::Result<Vec<Vertex>> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().parent_names(name)
    }

    fn all(&self) -> dag::Result<Set> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().all()
    }

    fn ancestors(&self, set: Set) -> dag::Result<Set> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().ancestors(set)
    }

    fn parents(&self, set: Set) -> dag::Result<Set> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().parents(set)
    }

    fn first_ancestor_nth(&self, name: Vertex, n: u64) -> dag::Result<Vertex> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().first_ancestor_nth(name, n)
    }

    fn heads(&self, set: Set) -> dag::Result<Set> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().heads(set)
    }

    fn children(&self, set: Set) -> dag::Result<Set> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().children(set)
    }

    fn roots(&self, set: Set) -> dag::Result<Set> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().roots(set)
    }

    fn gca_one(&self, set: Set) -> dag::Result<Option<Vertex>> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().gca_one(set)
    }

    fn gca_all(&self, set: Set) -> dag::Result<Set> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().gca_all(set)
    }

    fn common_ancestors(&self, set: Set) -> dag::Result<Set> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().common_ancestors(set)
    }

    fn is_ancestor(&self, ancestor: Vertex, descendant: Vertex) -> dag::Result<bool> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().is_ancestor(ancestor, descendant)
    }

    fn heads_ancestors(&self, set: Set) -> dag::Result<Set> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().heads_ancestors(set)
    }

    fn range(&self, roots: Set, heads: Set) -> dag::Result<Set> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().range(roots, heads)
    }

    fn only(&self, reachable: Set, unreachable: Set) -> dag::Result<Set> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().only(reachable, unreachable)
    }

    fn only_both(&self, reachable: Set, unreachable: Set) -> dag::Result<(Set, Set)> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().only_both(reachable, unreachable)
    }

    fn descendants(&self, set: Set) -> dag::Result<Set> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().descendants(set)
    }

    fn reachable_roots(&self, roots: Set, heads: Set) -> dag::Result<Set> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().reachable_roots(roots, heads)
    }

    fn snapshot_dag(&self) -> dag::Result<Arc<dyn DagAlgorithm + Send + Sync>> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().snapshot_dag()
    }
}

impl IdConvert for commits {
    fn vertex_id(&self, name: Vertex) -> dag::Result<Id> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().vertex_id(name)
    }
    fn vertex_id_with_max_group(&self, name: &Vertex, max_group: Group) -> dag::Result<Option<Id>> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py)
            .borrow()
            .vertex_id_with_max_group(name, max_group)
    }
    fn vertex_name(&self, id: Id) -> dag::Result<Vertex> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().vertex_name(id)
    }
    fn contains_vertex_name(&self, name: &Vertex) -> dag::Result<bool> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py).borrow().contains_vertex_name(name)
    }
}

impl PrefixLookup for commits {
    fn vertexes_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> dag::Result<Vec<Vertex>> {
        let py = unsafe { Python::assume_gil_acquired() };
        self.inner(py)
            .borrow()
            .vertexes_by_hex_prefix(hex_prefix, limit)
    }
}
