/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use anyhow::Error;
use cpython::*;
use cpython_ext::{AnyhowResultExt, PyNone, PyPath, ResultPyErrExt};
use dag::{
    id::{Id, VertexName},
    spanset::{SpanSet, SpanSetIter},
    NameDag,
};
use std::cell::RefCell;

use dag::namedag::LowLevelAccess;

type Result<T, E = Error> = std::result::Result<T, E>;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "dag"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<namedag>(py)?;
    m.add_class::<spans>(py)?;
    Ok(m)
}

/// A wrapper around [`SpanSet`] with Python integration.
///
/// Differences from the `py_class` version:
/// - Auto converts from a wider range of Python types - smartset, any iterator.
/// - No need to take the Python GIL to create a new instance of `Set`.
pub struct Spans(pub SpanSet);

impl Into<SpanSet> for Spans {
    fn into(self) -> SpanSet {
        self.0
    }
}

// Mercurial's special case. -1 maps to (b"\0" * 20)
const NULL_NODE: [u8; 20] = [0u8; 20];

// A wrapper around [`SpanSet`].
// This is different from `smartset.spanset`.
// Used in the Python world. The Rust world should use the `Spans` and `SpanSet` types.
py_class!(pub class spans |py| {
    data inner: SpanSet;

    def __new__(_cls, obj: PyObject) -> PyResult<spans> {
        Ok(Spans::extract(py, &obj)?.to_py_object(py))
    }

    def __contains__(&self, id: i64) -> PyResult<bool> {
        if id < 0 {
            Ok(false)
        } else {
            Ok(self.inner(py).contains(Id(id as u64)))
        }
    }

    def __len__(&self) -> PyResult<usize> {
        Ok(self.inner(py).count() as usize)
    }

    def __iter__(&self) -> PyResult<spansiter> {
        self.iterdesc(py)
    }

    def iterasc(&self) -> PyResult<spansiter> {
        let iter = RefCell::new( self.inner(py).clone().into_iter());
        spansiter::create_instance(py, iter, true)
    }

    def iterdesc(&self) -> PyResult<spansiter> {
        let iter = RefCell::new(self.inner(py).clone().into_iter());
        spansiter::create_instance(py, iter, false)
    }

    def min(&self) -> PyResult<Option<u64>> {
        Ok(self.inner(py).min().map(|id| id.0))
    }

    def max(&self) -> PyResult<Option<u64>> {
        Ok(self.inner(py).max().map(|id| id.0))
    }

    def __repr__(&self) -> PyResult<String> {
        Ok(format!("[{:?}]", self.inner(py)))
    }

    def __add__(lhs, rhs) -> PyResult<Spans> {
        let lhs = Spans::extract(py, lhs)?;
        let rhs = Spans::extract(py, rhs)?;
        Ok(Spans(lhs.0.union(&rhs.0)))
    }

    def __and__(lhs, rhs) -> PyResult<Spans> {
        let lhs = Spans::extract(py, lhs)?;
        let rhs = Spans::extract(py, rhs)?;
        Ok(Spans(lhs.0.intersection(&rhs.0)))
    }

    def __sub__(lhs, rhs) -> PyResult<Spans> {
        let lhs = Spans::extract(py, lhs)?;
        let rhs = Spans::extract(py, rhs)?;
        Ok(Spans(lhs.0.difference(&rhs.0)))
    }
});

// A wrapper to [`SpanSetIter`].
py_class!(pub class spansiter |py| {
    data iter: RefCell<SpanSetIter<SpanSet>>;
    data ascending: bool;

    def __next__(&self) -> PyResult<Option<u64>> {
        let mut iter = self.iter(py).borrow_mut();
        let next = if *self.ascending(py) {
            iter.next_back()
        } else {
            iter.next()
        };
        Ok(next.map(|id| id.0))
    }

    // Makes code like `list(spans.iterasc())` work.
    def __iter__(&self) -> PyResult<spansiter> {
        Ok(self.clone_ref(py))
    }
});

impl<'a> FromPyObject<'a> for Spans {
    fn extract(py: Python, obj: &'a PyObject) -> PyResult<Self> {
        // If obj already owns Set, then avoid iterating through it.
        if let Ok(pyset) = obj.extract::<spans>(py) {
            return Ok(Spans(pyset.inner(py).clone()));
        }

        // Try to call `sort(reverse=True)` on the object.
        // - Python smartset.baseset has sort(reverse=False) API.
        // - The Rust SpanSet is always sorted in reverse order internally.
        // - Most Python lazy smartsets (smartset.generatorset) are sorted in reverse order.
        if let Ok(sort) = obj.getattr(py, "sort") {
            let args = PyDict::new(py);
            args.set_item(py, "reverse", true)?;
            sort.call(py, NoArgs, Some(&args))?;
        }

        // Then iterate through obj and collect all ids.
        // Collecting ids to a Vec first to preserve error handling.
        let ids: PyResult<Vec<Id>> = obj
            .iter(py)?
            .map(|o| Ok(Id(o?.extract::<u64>(py)?)))
            .collect();
        Ok(Spans(SpanSet::from_spans(ids?)))
    }
}

impl ToPyObject for Spans {
    type ObjectType = spans;

    fn to_py_object(&self, py: Python) -> Self::ObjectType {
        spans::create_instance(py, self.0.clone()).unwrap()
    }
}

py_class!(class namedag |py| {
    data namedag: RefCell<NameDag>;
    data pyparentfunc: PyObject;

    def __new__(_cls, path: &PyPath, parentfunc: PyObject) -> PyResult<namedag> {
        let dag = NameDag::open(path.as_path()).map_pyerr(py)?;
        Self::create_instance(py, RefCell::new(dag), parentfunc)
    }

    /// Add heads to the in-memory DAG.
    def addheads(&self, heads: Vec<PyBytes>) -> PyResult<PyNone> {
        let mut namedag = self.namedag(py).borrow_mut();
        let parents = self.parentfunc(py);
        let heads = heads.into_iter().map(|b| VertexName::copy_from(b.data(py))).collect::<Vec<_>>();
        namedag.add_heads(&parents, &heads).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Write the DAG to disk.
    def flush(&self, masterheads: Vec<PyBytes>) -> PyResult<PyNone> {
        let mut namedag = self.namedag(py).borrow_mut();
        let heads = masterheads.into_iter().map(|b| VertexName::copy_from(b.data(py))).collect::<Vec<_>>();
        namedag.flush(&heads).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Write heads directly to disk. Similar to addheads + flush, but faster.
    def addheadsflush(&self, masterheads: Vec<PyBytes>, otherheads: Vec<PyBytes>) -> PyResult<PyNone> {
        let mut namedag = self.namedag(py).borrow_mut();
        let parents = self.parentfunc(py);
        let masterheads = masterheads.into_iter().map(|b| VertexName::copy_from(b.data(py))).collect::<Vec<_>>();
        let otherheads = otherheads.into_iter().map(|b| VertexName::copy_from(b.data(py))).collect::<Vec<_>>();
        namedag.add_heads_and_flush(&parents, &masterheads, &otherheads).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Translate id to node.
    def id2node(&self, id: i64) -> PyResult<Option<PyBytes>> {
        if id == -1 {
            Ok(Some(PyBytes::new(py, &NULL_NODE)))
        } else if id < 0 {
            Ok(None)
        } else {
            let namedag = self.namedag(py).borrow();
            Ok(namedag.map()
                .find_name_by_id(Id(id as u64))
                .map_pyerr(py)?
                .map(|node| PyBytes::new(py, node)))
        }
    }

    /// Translate node to id.
    def node2id(&self, node: PyBytes) -> PyResult<Option<i64>> {
        let node = node.data(py);
        if node == &NULL_NODE {
            Ok(Some(-1))
        } else {
            let namedag = self.namedag(py).borrow();
            Ok(namedag.map()
                .find_id_by_name(&node)
                .map_pyerr(py)?.map(|id| id.0 as i64))
        }
    }

    /// Lookup nodes by hex prefix.
    def hexprefixmatch(&self, prefix: PyBytes, limit: usize = 5) -> PyResult<Vec<PyBytes>> {
        let prefix = prefix.data(py);
        if !prefix.iter().all(|&b| (b >= b'0' && b <= b'9') || (b >= b'a' && b <= b'f')) {
            // Invalid hex prefix. Pretend nothing matches.
            return Ok(Vec::new())
        }
        let namedag = self.namedag(py).borrow();
        let nodes = namedag.map()
            .find_names_by_hex_prefix(prefix, limit)
            .map_pyerr(py)?
            .into_iter()
            .map(|s| PyBytes::new(py, &s))
            .collect();
        Ok(nodes)
    }

    def all(&self) -> PyResult<Spans> {
        let namedag = self.namedag(py).borrow();
        Ok(Spans(namedag.dag().all().map_pyerr(py)?))
    }

    /// Calculate all ancestors reachable from the set.
    def ancestors(&self, set: Spans) -> PyResult<Spans> {
        let namedag = self.namedag(py).borrow();
        Ok(Spans(namedag.dag().ancestors(set).map_pyerr(py)?))
    }

    /// Calculate parents of the given set.
    def parents(&self, set: Spans) -> PyResult<Spans> {
        let namedag = self.namedag(py).borrow();
        Ok(Spans(namedag.dag().parents(set).map_pyerr(py)?))
    }

    /// Get parents of a single `id`. Preserve the order.
    def parentids(&self, id: u64) -> PyResult<Vec<u64>> {
        let namedag = self.namedag(py).borrow();
        Ok(namedag.dag().parent_ids(Id(id)).map_pyerr(py)?.into_iter().map(|id| id.0).collect())
    }

    /// Calculate parents of the given set.
    def heads(&self, set: Spans) -> PyResult<Spans> {
        let namedag = self.namedag(py).borrow();
        Ok(Spans(namedag.dag().heads(set).map_pyerr(py)?))
    }

    /// Calculate children of the given set.
    def children(&self, set: Spans) -> PyResult<Spans> {
        let namedag = self.namedag(py).borrow();
        Ok(Spans(namedag.dag().children(set).map_pyerr(py)?))
    }

    /// Calculate roots of the given set.
    def roots(&self, set: Spans) -> PyResult<Spans> {
        let namedag = self.namedag(py).borrow();
        Ok(Spans(namedag.dag().roots(set).map_pyerr(py)?))
    }

    /// Calculate one greatest common ancestor of a set.
    /// If there are multiple greatest common ancestors, pick an arbitrary one.
    def gcaone(&self, set: Spans) -> PyResult<Option<u64>> {
        let namedag = self.namedag(py).borrow();
        Ok(namedag.dag().gca_one(set).map_pyerr(py)?.map(|id| id.0))
    }

    /// Calculate all greatest common ancestors of a set.
    def gcaall(&self, set: Spans) -> PyResult<Spans> {
        let namedag = self.namedag(py).borrow();
        Ok(Spans(namedag.dag().gca_all(set).map_pyerr(py)?))
    }

    /// Calculate all common ancestors of a set.
    def commonancestors(&self, set: Spans) -> PyResult<Spans> {
        let namedag = self.namedag(py).borrow();
        Ok(Spans(namedag.dag().common_ancestors(set).map_pyerr(py)?))
    }

    /// Check if `ancestor` is an ancestor of `descendant`.
    def isancestor(&self, ancestor: u64, descendant: u64) -> PyResult<bool> {
        let namedag = self.namedag(py).borrow();
        namedag.dag().is_ancestor(Id(ancestor), Id(descendant)).map_pyerr(py)
    }

    /// Calculate `heads(ancestors(set))`.
    /// This is faster than calling `heads` and `ancestors` individually.
    def headsancestors(&self, set: Spans) -> PyResult<Spans> {
        let namedag = self.namedag(py).borrow();
        Ok(Spans(namedag.dag().heads_ancestors(set).map_pyerr(py)?))
    }

    /// Calculate `roots::heads`.
    def range(&self, roots: Spans, heads: Spans) -> PyResult<Spans> {
        let namedag = self.namedag(py).borrow();
        Ok(Spans(namedag.dag().range(roots, heads).map_pyerr(py)?))
    }

    /// Calculate descendants of the given set.
    def descendants(&self, set: Spans) -> PyResult<Spans> {
        let namedag = self.namedag(py).borrow();
        Ok(Spans(namedag.dag().descendants(set).map_pyerr(py)?))
    }

    def debugsegments(&self) -> PyResult<String> {
        let namedag = self.namedag(py).borrow();
        Ok(format!("{:?}", namedag.dag()))
    }
});

impl namedag {
    /// Return the "parents" function that takes VertexName and returns
    /// VertexNames.
    fn parentfunc<'a>(
        &'a self,
        py: Python<'a>,
    ) -> impl Fn(VertexName) -> Result<Vec<VertexName>> + 'a {
        let pyparentfunc = self.pyparentfunc(py);
        move |node: VertexName| -> Result<Vec<VertexName>> {
            let mut result = Vec::new();
            let node = PyBytes::new(py, node.as_ref());
            let parents = pyparentfunc.call(py, (node,), None).into_anyhow_result()?;
            for parent in parents.iter(py).into_anyhow_result()? {
                let parent = VertexName::copy_from(
                    parent
                        .into_anyhow_result()?
                        .cast_as::<PyBytes>(py)
                        .map_err(PyErr::from)
                        .into_anyhow_result()?
                        .data(py),
                );
                result.push(parent);
            }
            Ok(result)
        }
    }
}
