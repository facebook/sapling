/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use anyhow::Error;
use cpython::*;
use cpython_ext::{AnyhowResultExt, PyNone, PyPath, ResultPyErrExt, Str};
use dag::{
    namedag::LowLevelAccess, nameset::id_static::IdStaticSet,
    nameset::legacy::LegacyCodeNeedIdAccess, ops::DagAddHeads, ops::DagPersistent,
    ops::PrefixLookup, spanset::SpanSetIter, Dag, DagAlgorithm, Id, IdSet, MemDag, Set, Vertex,
};
use std::cell::RefCell;
use std::ops::Deref;

mod nameset;

use nameset::Names;

type Result<T, E = Error> = std::result::Result<T, E>;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "dag"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<namedag>(py)?;
    m.add_class::<memnamedag>(py)?;
    m.add_class::<spans>(py)?;
    m.add_class::<nameset::nameset>(py)?;
    Ok(m)
}

/// A wrapper around [`IdSet`] with Python integration.
///
/// Differences from the `py_class` version:
/// - Auto converts from a wider range of Python types - smartset, any iterator.
/// - No need to take the Python GIL to create a new instance of `Set`.
pub struct Spans(pub IdSet);

impl Into<IdSet> for Spans {
    fn into(self) -> IdSet {
        self.0
    }
}

// Mercurial's special case. -1 maps to (b"\0" * 20)
const NULL_NODE: [u8; 20] = [0u8; 20];

// A wrapper around [`IdSet`].
// This is different from `smartset.spanset`.
// Used in the Python world. The Rust world should use the `Spans` and `IdSet` types.
py_class!(pub class spans |py| {
    data inner: IdSet;

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
    data iter: RefCell<SpanSetIter<IdSet>>;
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
        // - The Rust IdSet is always sorted in reverse order internally.
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
        Ok(Spans(IdSet::from_spans(ids?)))
    }
}

impl ToPyObject for Spans {
    type ObjectType = spans;

    fn to_py_object(&self, py: Python) -> Self::ObjectType {
        spans::create_instance(py, self.0.clone()).unwrap()
    }
}

py_class!(class namedag |py| {
    data namedag: RefCell<Dag>;

    def __new__(_cls, path: &PyPath) -> PyResult<namedag> {
        let dag = Dag::open(path.as_path()).map_pyerr(py)?;
        Self::create_instance(py, RefCell::new(dag))
    }

    /// Add heads to the in-memory DAG.
    def addheads(&self, heads: Vec<PyBytes>, parentfunc: PyObject) -> PyResult<PyNone> {
        let mut namedag = self.namedag(py).borrow_mut();
        let parents = wrap_parentfunc(py, parentfunc);
        let heads = heads.into_iter().map(|b| Vertex::copy_from(b.data(py))).collect::<Vec<_>>();
        namedag.add_heads(&parents, &heads).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Write the DAG to disk.
    def flush(&self, masterheads: Vec<PyBytes>) -> PyResult<PyNone> {
        let mut namedag = self.namedag(py).borrow_mut();
        let heads = masterheads.into_iter().map(|b| Vertex::copy_from(b.data(py))).collect::<Vec<_>>();
        namedag.flush(&heads).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Write heads directly to disk. Similar to addheads + flush, but faster.
    def addheadsflush(&self, masterheads: Vec<PyBytes>, otherheads: Vec<PyBytes>, parentfunc: PyObject) -> PyResult<PyNone> {
        let mut namedag = self.namedag(py).borrow_mut();
        let parents = wrap_parentfunc(py, parentfunc);
        let masterheads = masterheads.into_iter().map(|b| Vertex::copy_from(b.data(py))).collect::<Vec<_>>();
        let otherheads = otherheads.into_iter().map(|b| Vertex::copy_from(b.data(py))).collect::<Vec<_>>();
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

    /// Translate a set using names to using ids. This is similar to `node2id` but works for a set.
    /// Ideally this API does not exist. However the revset layer heavily uses ids.
    def node2idset(&self, set: Names) -> PyResult<Spans> {
        let set: Set = set.0;
        if let Some(set) = set.as_any().downcast_ref::<IdStaticSet>() {
            let spans: IdSet = (LegacyCodeNeedIdAccess, set).into();
            Ok(Spans(spans))
        } else {
            let namedag = self.namedag(py).borrow();
            let set = namedag.sort(&set).map_pyerr(py)?;
            let set = set.as_any().downcast_ref::<IdStaticSet>().expect("namedag.sort should return IdStaticSet");
            let spans: IdSet = (LegacyCodeNeedIdAccess, set).into();
            Ok(Spans(spans))
        }
    }

    /// Translate a set using ids to using names. This is similar to `id2node` but works for a set.
    /// Ideally this API does not exist. However the revset layer heavily uses ids.
    def id2nodeset(&self, set: Spans) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        let spans: IdSet = set.0;
        let set: Set = (LegacyCodeNeedIdAccess, spans , namedag.deref()).into();
        Ok(Names(set))
    }

    /// Lookup nodes by hex prefix.
    def hexprefixmatch(&self, prefix: PyBytes, limit: usize = 5) -> PyResult<Vec<PyBytes>> {
        let prefix = prefix.data(py);
        if !prefix.iter().all(|&b| (b >= b'0' && b <= b'9') || (b >= b'a' && b <= b'f')) {
            // Invalid hex prefix. Pretend nothing matches.
            return Ok(Vec::new())
        }
        let namedag = self.namedag(py).borrow();
        let nodes = namedag
            .vertexes_by_hex_prefix(prefix, limit)
            .map_pyerr(py)?
            .into_iter()
            .map(|s| PyBytes::new(py, s.as_ref()))
            .collect();
        Ok(nodes)
    }

    /// Sort a set.
    def sort(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.sort(&set.0).map_pyerr(py)?))
    }

    def all(&self) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.all().map_pyerr(py)?))
    }

    /// Calculate all ancestors reachable from the set.
    def ancestors(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.ancestors(set.0).map_pyerr(py)?))
    }

    /// Calculate parents of the given set.
    def parents(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.parents(set.0).map_pyerr(py)?))
    }

    /// Get parents of a single `name`. Preserve the order.
    def parentnames(&self, name: PyBytes) -> PyResult<Vec<PyBytes>> {
        let namedag = self.namedag(py).borrow();
        let parents = namedag.parent_names(Vertex::copy_from(name.data(py))).map_pyerr(py)?;
        Ok(parents.into_iter().map(|name| PyBytes::new(py, name.as_ref())).collect())
    }

    /// Calculate parents of the given set.
    def heads(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.heads(set.0).map_pyerr(py)?))
    }

    /// Calculate children of the given set.
    def children(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.children(set.0).map_pyerr(py)?))
    }

    /// Calculate roots of the given set.
    def roots(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.roots(set.0).map_pyerr(py)?))
    }

    /// Calculate one greatest common ancestor of a set.
    /// If there are multiple greatest common ancestors, pick an arbitrary one.
    def gcaone(&self, set: Names) -> PyResult<Option<PyBytes>> {
        let namedag = self.namedag(py).borrow();
        Ok(namedag.gca_one(set.0).map_pyerr(py)?.map(|name| PyBytes::new(py, name.as_ref())))
    }

    /// Calculate all greatest common ancestors of a set.
    def gcaall(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.gca_all(set.0).map_pyerr(py)?))
    }

    /// Calculate all common ancestors of a set.
    def commonancestors(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.common_ancestors(set.0).map_pyerr(py)?))
    }

    /// Check if `ancestor` is an ancestor of `descendant`.
    def isancestor(&self, ancestor: PyBytes, descendant: PyBytes) -> PyResult<bool> {
        let namedag = self.namedag(py).borrow();
        let ancestor = Vertex::copy_from(ancestor.data(py));
        let descendant = Vertex::copy_from(descendant.data(py));
        namedag.is_ancestor(ancestor, descendant).map_pyerr(py)
    }

    /// Calculate `heads(ancestors(set))`.
    /// This is faster than calling `heads` and `ancestors` individually.
    def headsancestors(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.heads_ancestors(set.0).map_pyerr(py)?))
    }

    /// Calculate `roots::heads`.
    def range(&self, roots: Names, heads: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.range(roots.0, heads.0).map_pyerr(py)?))
    }

    /// Calculate descendants of the given set.
    def descendants(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.descendants(set.0).map_pyerr(py)?))
    }

    def debugsegments(&self) -> PyResult<String> {
        let namedag = self.namedag(py).borrow();
        Ok(format!("{:?}", namedag.dag()))
    }
});

py_class!(pub class memnamedag |py| {
    data namedag: RefCell<MemDag>;

    def __new__(_cls) -> PyResult<Self> {
        let dag = MemDag::new();
        Self::create_instance(py, RefCell::new(dag))
    }

    /// Add heads to the in-memory DAG.
    def addheads(&self, heads: Vec<PyBytes>, parentfunc: PyObject) -> PyResult<PyNone> {
        let mut namedag = self.namedag(py).borrow_mut();
        let parents = wrap_parentfunc(py, parentfunc);
        let heads = heads.into_iter().map(|b| Vertex::copy_from(b.data(py))).collect::<Vec<_>>();
        namedag.add_heads(&parents, &heads).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Sort a set.
    def sort(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.sort(&set.0).map_pyerr(py)?))
    }

    def all(&self) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.all().map_pyerr(py)?))
    }

    /// Calculate all ancestors reachable from the set.
    def ancestors(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.ancestors(set.0).map_pyerr(py)?))
    }

    /// Calculate parents of the given set.
    def parents(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.parents(set.0).map_pyerr(py)?))
    }

    /// Get parents of a single `name`. Preserve the order.
    def parentnames(&self, name: PyBytes) -> PyResult<Vec<PyBytes>> {
        let namedag = self.namedag(py).borrow();
        let parents = namedag.parent_names(Vertex::copy_from(name.data(py))).map_pyerr(py)?;
        Ok(parents.into_iter().map(|name| PyBytes::new(py, name.as_ref())).collect())
    }

    /// Calculate parents of the given set.
    def heads(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.heads(set.0).map_pyerr(py)?))
    }

    /// Calculate children of the given set.
    def children(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.children(set.0).map_pyerr(py)?))
    }

    /// Calculate roots of the given set.
    def roots(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.roots(set.0).map_pyerr(py)?))
    }

    /// Calculate one greatest common ancestor of a set.
    /// If there are multiple greatest common ancestors, pick an arbitrary one.
    def gcaone(&self, set: Names) -> PyResult<Option<PyBytes>> {
        let namedag = self.namedag(py).borrow();
        Ok(namedag.gca_one(set.0).map_pyerr(py)?.map(|name| PyBytes::new(py, name.as_ref())))
    }

    /// Calculate all greatest common ancestors of a set.
    def gcaall(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.gca_all(set.0).map_pyerr(py)?))
    }

    /// Calculate all common ancestors of a set.
    def commonancestors(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.common_ancestors(set.0).map_pyerr(py)?))
    }

    /// Check if `ancestor` is an ancestor of `descendant`.
    def isancestor(&self, ancestor: PyBytes, descendant: PyBytes) -> PyResult<bool> {
        let namedag = self.namedag(py).borrow();
        let ancestor = Vertex::copy_from(ancestor.data(py));
        let descendant = Vertex::copy_from(descendant.data(py));
        namedag.is_ancestor(ancestor, descendant).map_pyerr(py)
    }

    /// Calculate `heads(ancestors(set))`.
    /// This is faster than calling `heads` and `ancestors` individually.
    def headsancestors(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.heads_ancestors(set.0).map_pyerr(py)?))
    }

    /// Calculate `roots::heads`.
    def range(&self, roots: Names, heads: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.range(roots.0, heads.0).map_pyerr(py)?))
    }

    /// Calculate descendants of the given set.
    def descendants(&self, set: Names) -> PyResult<Names> {
        let namedag = self.namedag(py).borrow();
        Ok(Names(namedag.descendants(set.0).map_pyerr(py)?))
    }

    /// Render the graph into an ASCII string.
    def render(&self, getmessage: Option<PyObject> = None) -> PyResult<Str> {
        let namedag = self.namedag(py).borrow();
        let get_message = move |vertex: &Vertex| -> Option<String> {
            if let Some(getmessage) = &getmessage {
                if getmessage.is_callable(py) {
                    if let Ok(message) = getmessage.call(py, (PyBytes::new(py, vertex.as_ref()),), None) {
                        if let Ok(message) = message.extract::<String>(py) {
                            return Some(message)
                        }
                    }
                }
            }
            None
        };
        Ok(renderdag::render_namedag(namedag.deref(), get_message).map_pyerr(py)?.into())
    }

    /// Beautify the graph so `render` might look better.
    def beautify(&self, mainbranch: Option<Names> = None) -> PyResult<Self> {
        let namedag = self.namedag(py).borrow();
        let dag = namedag.beautify(mainbranch.map(|h| h.0)).map_pyerr(py)?;
        Self::from_memnamedag(py, dag)
    }
});

impl memnamedag {
    pub fn from_memnamedag(py: Python, dag: MemDag) -> PyResult<Self> {
        Self::create_instance(py, RefCell::new(dag))
    }
}

/// Return the "parents" function that takes Vertex and returns
/// Vertexs.
fn wrap_parentfunc<'a>(
    py: Python<'a>,
    pyparentfunc: PyObject,
) -> impl Fn(Vertex) -> Result<Vec<Vertex>> + 'a {
    move |node: Vertex| -> Result<Vec<Vertex>> {
        let mut result = Vec::new();
        let node = PyBytes::new(py, node.as_ref());
        let parents = pyparentfunc.call(py, (node,), None).into_anyhow_result()?;
        for parent in parents.iter(py).into_anyhow_result()? {
            let parent = Vertex::copy_from(
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
