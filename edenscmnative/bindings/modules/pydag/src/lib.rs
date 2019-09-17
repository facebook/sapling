// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![allow(non_camel_case_types)]

use cpython::*;
use cpython_failure::{FallibleExt, ResultPyErrExt};
use dag::{
    idmap::{Id, IdMap},
    segment::Dag,
    spanset::SpanSet,
};
use encoding::local_bytes_to_path;
use failure::Fallible;
use std::cell::RefCell;

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
pub(crate) struct Spans(pub SpanSet);

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

    def __contains__(&self, id: Id) -> PyResult<bool> {
        Ok(self.inner(py).contains(id))
    }

    def __len__(&self) -> PyResult<usize> {
        Ok(self.inner(py).count() as usize)
    }

    def __iter__(&self) -> PyResult<PyObject> {
        let ids: Vec<Id> = self.inner(py).iter().collect();
        let list: PyList = ids.into_py_object(py);
        list.into_object().call_method(py, "__iter__", NoArgs, None)
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
        let ids: PyResult<Vec<Id>> = obj.iter(py)?.map(|o| o?.extract(py)).collect();
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
    def builddisk(&self, nodes: Vec<PyBytes>, parentfunc: PyObject) -> PyResult<Option<u8>> {
        // Build indexes towards `node`. Save state on disk.
        // Must be called from a clean state (ex. `build_mem` is not called).
        if nodes.is_empty() {
            return Ok(None);
        }
        let get_parents = translate_get_parents(py, parentfunc);
        let mut map = self.map(py).borrow_mut();
        let id = {
            let mut map = map.prepare_filesystem_sync().map_pyerr::<exc::IOError>(py)?;
            let mut id = 0;
            for node in nodes {
                let node = node.data(py);
                id = id.max(map.assign_head(&node, &get_parents).map_pyerr::<exc::RuntimeError>(py)?);
            }
            map.sync().map_pyerr::<exc::IOError>(py)?;
            id
        };
        let get_parents = map.build_get_parents_by_id(&get_parents);

        let mut dag = self.dag(py).borrow_mut();
        {
            use std::ops::DerefMut;
            let mut syncable = dag.prepare_filesystem_sync().map_pyerr::<exc::IOError>(py)?;
            syncable.build_segments_persistent(id, &get_parents).map_pyerr::<exc::IOError>(py)?;
            syncable.sync(std::iter::once(dag.deref_mut())).map_pyerr::<exc::IOError>(py)?;
        }
        Ok(None)
    }

    /// Build segments in memory. Note: This gets discarded by `builddisk`.
    def buildmem(&self, nodes: Vec<PyBytes>, parentfunc: PyObject) -> PyResult<Option<u8>> {
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
                id = id.max(map.assign_head(&node, &get_parents).map_pyerr::<exc::RuntimeError>(py)?);
            }
            id
        };
        let get_parents = map.build_get_parents_by_id(&get_parents);

        let mut dag = self.dag(py).borrow_mut();
        dag.build_segments_volatile(id, &get_parents).map_pyerr::<exc::IOError>(py)?;
        Ok(None)
    }

    def id2node(&self, id: Id) -> PyResult<Option<PyBytes>> {
        // Translate id to node.
        let map = self.map(py).borrow();
        Ok(map
            .find_slice_by_id(id)
            .map_pyerr::<exc::IOError>(py)?
            .map(|node| PyBytes::new(py, node)))
    }

    def node2id(&self, node: PyBytes) -> PyResult<Option<Id>> {
        // Translate node to id.
        let node = node.data(py);
        let map = self.map(py).borrow();
        Ok(map
            .find_id_by_slice(&node)
            .map_pyerr::<exc::IOError>(py)?)
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
    def gcaone(&self, set: Spans) -> PyResult<Option<Id>> {
        let dag = self.dag(py).borrow();
        dag.gca_one(set).map_pyerr::<exc::IOError>(py)
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
    def isancestor(&self, ancestor: Id, descentant: Id) -> PyResult<bool> {
        let dag = self.dag(py).borrow();
        dag.is_ancestor(ancestor, descentant).map_pyerr::<exc::IOError>(py)
    }

    /// Calculate `heads(ancestors(set))`.
    /// This is faster than calling `heads` and `ancestors` individually.
    def headsancestors(&self, set: Spans) -> PyResult<Spans> {
        let dag = self.dag(py).borrow();
        Ok(Spans(dag.common_ancestors(set).map_pyerr::<exc::IOError>(py)?))
    }
});

/// Translate a Python `get_parents(node) -> [node]` function to a Rust one.
fn translate_get_parents<'a>(
    py: Python<'a>,
    get_parents: PyObject,
) -> impl Fn(&[u8]) -> Fallible<Vec<Box<[u8]>>> + 'a {
    move |node: &[u8]| -> Fallible<Vec<Box<[u8]>>> {
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
