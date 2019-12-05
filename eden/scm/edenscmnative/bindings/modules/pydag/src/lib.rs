/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use anyhow::Error;
use cpython::*;
use cpython_failure::{FallibleExt, ResultPyErrExt};
use dag::{
    id::{GroupId, Id},
    idmap::IdMap,
    segment::Dag,
    spanset::{SpanSet, SpanSetIter},
};
use encoding::local_bytes_to_path;
use std::cell::RefCell;

type Result<T, E = Error> = std::result::Result<T, E>;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "dag"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<dagindex>(py)?;
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

py_class!(class dagindex |py| {
    data dag: RefCell<Dag>;
    data map: RefCell<IdMap>;
    data segment_size: usize;
    data max_segment_level: u8;

    def __new__(_cls, path: &PyBytes, segment_size: usize = 16, max_segment_level: u8 = 4) -> PyResult<dagindex> {
        assert!(segment_size > 0);
        let path = local_bytes_to_path(path.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
        let dag = Dag::open(path.join("segment")).map_pyerr::<exc::IOError>(py)?;
        let map = IdMap::open(path.join("idmap")).map_pyerr::<exc::IOError>(py)?;
        Self::create_instance(py, RefCell::new(dag), RefCell::new(map), segment_size, max_segment_level)
    }

    /// Build segments on disk. This discards changes by `buildmem`.
    def builddisk(&self, masternodes: Vec<PyBytes>, othernodes: Vec<PyBytes>, parentfunc: PyObject) -> PyResult<Option<u8>> {
        // Build indexes towards `node`. Save state on disk.
        // Must be called from a clean state (ex. `build_mem` is not called).
        if masternodes.is_empty() && othernodes.is_empty() {
            return Ok(None);
        }
        let get_parents = translate_get_parents(py, parentfunc);
        let mut map = self.map(py).borrow_mut();
        let mut map = map.prepare_filesystem_sync().map_pyerr::<exc::IOError>(py)?;
        for (nodes, group) in [(masternodes, GroupId::MASTER), (othernodes, GroupId::NON_MASTER)].iter() {
            for node in nodes {
                let node = node.data(py);
                map.assign_head(&node, &get_parents, *group).map_pyerr::<exc::RuntimeError>(py)?;
            }
        }
        map.sync().map_pyerr::<exc::IOError>(py)?;

        let get_parents = map.build_get_parents_by_id(&get_parents);
        let mut dag = self.dag(py).borrow_mut();
        use std::ops::DerefMut;
        let mut syncable = dag.prepare_filesystem_sync().map_pyerr::<exc::IOError>(py)?;
        for &group in GroupId::ALL.iter() {
            let id = map.next_free_id(group).map_pyerr::<exc::IOError>(py)?;
            if id > group.min_id() {
                syncable.build_segments_persistent(id - 1, &get_parents).map_pyerr::<exc::IOError>(py)?;
            }
        }
        syncable.sync(std::iter::once(dag.deref_mut())).map_pyerr::<exc::IOError>(py)?;
        Ok(None)
    }

    /// Build segments in memory. Note: This gets discarded by `builddisk`.
    def buildmem(&self, nodes: Vec<PyBytes>, parentfunc: PyObject, master: bool = true) -> PyResult<Option<u8>> {
        // Build indexes towards `node`. Do not save state to disk.
        if nodes.is_empty() {
            return Ok(None);
        }
        let get_parents = translate_get_parents(py, parentfunc);
        let mut map = self.map(py).borrow_mut();
        let id = {
            let mut id = 0;
            for node in nodes {
                let node = node.data(py);
                id = id.max(map.assign_head(&node, &get_parents, group_id(master)).map_pyerr::<exc::RuntimeError>(py)?.0);
            }
            id
        };
        let get_parents = map.build_get_parents_by_id(&get_parents);

        let mut dag = self.dag(py).borrow_mut();
        dag.build_segments_volatile(Id(id), &get_parents).map_pyerr::<exc::IOError>(py)?;
        Ok(None)
    }

    def id2node(&self, id: u64) -> PyResult<Option<PyBytes>> {
        // Translate id to node.
        let map = self.map(py).borrow();
        Ok(map
            .find_slice_by_id(Id(id))
            .map_pyerr::<exc::IOError>(py)?
            .map(|node| PyBytes::new(py, node)))
    }

    def node2id(&self, node: PyBytes) -> PyResult<Option<u64>> {
        // Translate node to id.
        let node = node.data(py);
        let map = self.map(py).borrow();
        Ok(map
            .find_id_by_slice(&node)
            .map_pyerr::<exc::IOError>(py)?.map(|id| id.0))
    }

    /// Calculate all ancestors reachable from the set.
    def ancestors(&self, set: Spans) -> PyResult<Spans> {
        let dag = self.dag(py).borrow();
        Ok(Spans(dag.ancestors(set).map_pyerr::<exc::IOError>(py)?))
    }

    /// Calculate parents of the given set.
    def parents(&self, set: Spans) -> PyResult<Spans> {
        let dag = self.dag(py).borrow();
        Ok(Spans(dag.parents(set).map_pyerr::<exc::IOError>(py)?))
    }

    /// Calculate parents of the given set.
    def heads(&self, set: Spans) -> PyResult<Spans> {
        let dag = self.dag(py).borrow();
        Ok(Spans(dag.heads(set).map_pyerr::<exc::IOError>(py)?))
    }

    /// Calculate one greatest common ancestor of a set.
    /// If there are multiple greatest common ancestors, pick an arbitrary one.
    def gcaone(&self, set: Spans) -> PyResult<Option<u64>> {
        let dag = self.dag(py).borrow();
        Ok(dag.gca_one(set).map_pyerr::<exc::IOError>(py)?.map(|id| id.0))
    }

    /// Calculate all greatest common ancestors of a set.
    def gcaall(&self, set: Spans) -> PyResult<Spans> {
        let dag = self.dag(py).borrow();
        Ok(Spans(dag.gca_all(set).map_pyerr::<exc::IOError>(py)?))
    }

    /// Calculate all common ancestors of a set.
    def commonancestors(&self, set: Spans) -> PyResult<Spans> {
        let dag = self.dag(py).borrow();
        Ok(Spans(dag.common_ancestors(set).map_pyerr::<exc::IOError>(py)?))
    }

    /// Check if `ancestor` is an ancestor of `descentant`.
    def isancestor(&self, ancestor: u64, descentant: u64) -> PyResult<bool> {
        let dag = self.dag(py).borrow();
        dag.is_ancestor(Id(ancestor), Id(descentant)).map_pyerr::<exc::IOError>(py)
    }

    /// Calculate `heads(ancestors(set))`.
    /// This is faster than calling `heads` and `ancestors` individually.
    def headsancestors(&self, set: Spans) -> PyResult<Spans> {
        let dag = self.dag(py).borrow();
        Ok(Spans(dag.common_ancestors(set).map_pyerr::<exc::IOError>(py)?))
    }
});

fn group_id(master: bool) -> GroupId {
    if master {
        GroupId::MASTER
    } else {
        GroupId::NON_MASTER
    }
}

/// Translate a Python `get_parents(node) -> [node]` function to a Rust one.
fn translate_get_parents<'a>(
    py: Python<'a>,
    get_parents: PyObject,
) -> impl Fn(&[u8]) -> Result<Vec<Box<[u8]>>> + 'a {
    move |node: &[u8]| -> Result<Vec<Box<[u8]>>> {
        let mut result = Vec::new();
        let node = PyBytes::new(py, node);
        let parents = get_parents.call(py, (node,), None).into_fallible()?;
        for parent in parents.iter(py).into_fallible()? {
            let parent = parent
                .into_fallible()?
                .cast_as::<PyBytes>(py)
                .map_err(PyErr::from)
                .into_fallible()?
                .data(py)
                .to_vec()
                .into_boxed_slice();
            result.push(parent);
        }
        Ok(result)
    }
}
