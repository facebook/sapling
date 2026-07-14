/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::linelog::CheckoutRev;
use ::linelog::EditFlags;
use ::linelog::EntryId;
use ::linelog::NanoDag as NativeNanoDag;
use ::linelog::SmallRevs as NativeSmallRevs;
use cpython::*;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "linelog"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<IntLineLog>(py)?;
    m.add_class::<NanoDag>(py)?;
    m.add_class::<SmallRevs>(py)?;
    Ok(m)
}

// Line content is "int", not "str".
type NativeIntLineLog = ::linelog::AbstractLineLog<usize>;

// Compact set of revision numbers used by linelog.
//
// SmallRevs behaves like an immutable Python object: methods such as insert(),
// union(), difference(), and intersection() return a new SmallRevs instead of
// modifying self.
py_class!(class SmallRevs |py| {
    data inner: NativeSmallRevs;

    def __new__(_cls, revs: Option<Vec<usize>> = None) -> PyResult<Self> {
        let inner = revs.unwrap_or_default().into_iter().collect();
        Self::create_instance(py, inner)
    }

    def __contains__(&self, rev: usize) -> PyResult<bool> {
        Ok(self.inner(py).contains(rev))
    }

    def __bool__(&self) -> PyResult<bool> {
        Ok(!self.inner(py).is_empty())
    }

    def __repr__(&self) -> PyResult<String> {
        Ok(format!("{:?}", self.inner(py)))
    }

    /// Return a new SmallRevs containing rev in addition to the current set.
    def insert(&self, rev: usize) -> PyResult<Self> {
        let mut inner = self.inner(py).clone();
        inner.insert(rev);
        Self::create_instance(py, inner)
    }

    /// Return the union of self and other.
    def union(&self, other: SmallRevs) -> PyResult<Self> {
        let mut inner = self.inner(py).clone();
        inner.union_with(other.inner(py));
        Self::create_instance(py, inner)
    }

    /// Return the set difference: self - other.
    def difference(&self, other: SmallRevs) -> PyResult<Self> {
        let mut inner = self.inner(py).clone();
        inner.difference_with(other.inner(py));
        Self::create_instance(py, inner)
    }

    /// Return the intersection of self and other.
    def intersection(&self, other: SmallRevs) -> PyResult<Self> {
        let mut inner = self.inner(py).clone();
        inner.intersect_with(other.inner(py));
        Self::create_instance(py, inner)
    }

    /// Return revisions in ascending order.
    def to_list(&self) -> PyResult<Vec<usize>> {
        Ok(self.inner(py).iter().collect())
    }
});

// Small immutable DAG for linelog revision ordering.
//
// Edges are parent -> child, and parent must be <= child. Query methods return
// empty collections for out-of-bound revisions. Ancestor and descendant sets
// include the queried revision itself.
py_class!(class NanoDag |py| {
    data inner: NativeNanoDag;

    def __new__(_cls) -> PyResult<Self> {
        Self::create_instance(py, Default::default())
    }

    /// Return parent revisions for rev, or an empty list if rev is out of bound.
    def parents(&self, rev: usize) -> PyResult<Vec<usize>> {
        Ok(self.inner(py).parents(rev).to_vec())
    }

    def __contains__(&self, rev: usize) -> PyResult<bool> {
        Ok(rev < self.inner(py).len())
    }

    def __repr__(&self) -> PyResult<String> {
        Ok(format!("<{:?}>", self.inner(py)))
    }

    /// Return ancestors of rev, including rev itself, or an empty set if out of bound.
    def ancestors(&self, rev: usize) -> PyResult<SmallRevs> {
        SmallRevs::create_instance(py, self.inner(py).ancestors(rev).clone())
    }

    /// Return descendants of rev, including rev itself, or an empty set if out of bound.
    def descendants(&self, rev: usize) -> PyResult<SmallRevs> {
        SmallRevs::create_instance(py, self.inner(py).descendants(rev).clone())
    }

    /// Return True if ancestor is an ancestor of descendant.
    ///
    /// A revision is considered an ancestor of itself.
    def is_ancestor(&self, ancestor: usize, descendant: usize) -> PyResult<bool> {
        Ok(self.inner(py).is_ancestor(ancestor, descendant))
    }

    /// Return a new NanoDag with the parent -> child edge added.
    ///
    /// If parent == child, this only ensures child exists in the DAG. Raises
    /// ValueError if parent > child.
    def with_edge(&self, parent: usize, child: usize) -> PyResult<Self> {
        if parent > child {
            return Err(PyErr::new::<exc::ValueError, _>(
                py,
                "parent revision must not be greater than child revision",
            ));
        }
        let inner = self.inner(py).clone().with_edge(parent, child);
        Self::create_instance(py, inner)
    }
});

py_class!(class IntLineLog |py| {
    data inner: NativeIntLineLog;

    def __new__(_cls) -> PyResult<Self> {
        Self::create_instance(py, Default::default())
    }

    /// Get the maximum rev (inclusive).
    def max_rev(&self) -> PyResult<usize> {
        Ok(self.inner(py).max_rev())
    }

    /// Edit chunk (a_rev, a1, a2, b_rev, b1, b2) -> self.
    def edit_chunk(&self, a_rev: usize, a1: usize, a2: usize, b_rev: usize, b1: usize, b2: usize, entry: usize = 0) -> PyResult<Self> {
        let inner = self.inner(py);
        let b_lines = (b1..b2).collect::<Vec<_>>();
        // BLOCK_SHIFT compares lines. However, IntLineLog uses pure line numbers without revision
        // numbers. So rev X line K can match rev Y line K (K == K), but the actual line content
        // might not match. Disable BLOCK_SHIFT to maintain correctness.
        let flags = EditFlags::default() - EditFlags::BLOCK_SHIFT;
        let new_value = inner.clone().edit_chunk(EntryId(entry), a_rev, a1, a2, b_rev, b_lines, flags);
        Self::create_instance(py, new_value)
    }

    /// Add an edge to the dag.
    def with_dag_edge(&self, a_rev: usize, b_rev: usize) -> PyResult<Self> {
        let inner = self.inner(py);
        let new_value = inner.clone().with_dag_edge(a_rev, b_rev);
        Self::create_instance(py, new_value)
    }

    /// Get the dag associated with this linelog.
    def nanodag(&self) -> PyResult<NanoDag> {
        NanoDag::create_instance(py, self.inner(py).nanodag().clone())
    }

    /// Get the lines. rev -> [(rev, line_no, pc, deleted)].
    /// Includes a dummy "end" line at the end.
    def checkout_lines(&self, rev: usize, entry: usize = 0) -> PyResult<Vec<(usize, usize, usize, bool)>> {
        let inner = self.inner(py);
        let lines = inner.checkout_lines(EntryId(entry), rev);
        let lines: Vec<_> = lines.into_iter().map(|l| (l.rev, *l.data.as_ref(), l.pc, l.deleted)).collect();
        Ok(lines)
    }

    /// Get the lines visible in the revs set. revs -> [(rev, line_no, pc, deleted)].
    /// Includes a dummy "end" line at the end.
    def checkout_revs_lines(&self, revs: SmallRevs, entry: usize = 0) -> PyResult<Vec<(usize, usize, usize, bool)>> {
        let inner = self.inner(py);
        let lines = inner.checkout_lines(EntryId(entry), CheckoutRev::Range(revs.inner(py).clone()));
        let lines: Vec<_> = lines.into_iter().map(|l| (l.rev, *l.data.as_ref(), l.pc, l.deleted)).collect();
        Ok(lines)
    }
});
