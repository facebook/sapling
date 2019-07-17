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
};
use encoding::local_bytes_to_path;
use failure::Fallible;
use std::cell::RefCell;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "dag"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<dagindex>(py)?;
    Ok(m)
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

    def build_disk(&self, nodes: Vec<PyBytes>, parent_func: PyObject) -> PyResult<Option<u8>> {
        // Build indexes towards `node`. Save state on disk.
        // Must be called from a clean state (ex. `build_mem` is not called).
        if nodes.is_empty() {
            return Ok(None);
        }
        let get_parents = translate_get_parents(py, parent_func);
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
            let mut dag = dag.prepare_filesystem_sync().map_pyerr::<exc::IOError>(py)?;
            dag.build_flat_segments(id, &get_parents, 0).map_pyerr::<exc::IOError>(py)?;
            let segment_size = *self.segment_size(py);
            for level in 1..=*self.max_segment_level(py) {
                dag.build_high_level_segments(level, segment_size, true).map_pyerr::<exc::IOError>(py)?;
            }
            dag.sync().map_pyerr::<exc::IOError>(py)?;
        }
        Ok(None)
    }

    def build_mem(&self, nodes: Vec<PyBytes>, parent_func: PyObject) -> PyResult<Option<u8>> {
        // Build indexes towards `node`. Do not save state to disk.
        if nodes.is_empty() {
            return Ok(None);
        }
        let get_parents = translate_get_parents(py, parent_func);
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
        dag.build_flat_segments(id, &get_parents, 0).map_pyerr::<exc::IOError>(py)?;
        let segment_size = *self.segment_size(py);
        for level in 1..=*self.max_segment_level(py) {
            dag.build_high_level_segments(level, segment_size, false).map_pyerr::<exc::IOError>(py)?;
        }
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

    def ancestor(&self, a: PyBytes, b: PyBytes) -> PyResult<Option<PyBytes>> {
        // Calculate ancestor of two nodes.
        let map = self.map(py).borrow();

        let a = map.find_id_by_slice(&a.data(py)).map_pyerr::<exc::IOError>(py)?;
        let b = map.find_id_by_slice(&b.data(py)).map_pyerr::<exc::IOError>(py)?;

        Ok(match (a, b) {
            (Some(a), Some(b)) => {
                let dag = self.dag(py).borrow();
                dag.ancestor(a, b).map_pyerr::<exc::IOError>(py)?.map(|id| {
                    let node = map.find_slice_by_id(id).unwrap().unwrap();
                    PyBytes::new(py, node)
                })
            }
            _ => None,
        })
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
