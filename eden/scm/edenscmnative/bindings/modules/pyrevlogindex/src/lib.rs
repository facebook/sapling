/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::path::Path;

use ::revlogindex::RevlogIndex;
use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::ResultPyErrExt;
use pydag::Spans;

// XXX: The revlogindex is a temporary solution before migrating to
// segmented changelog. It is here to experiment breaking changes with
// revlog, incluing:
//
// - Redefine "head()" to only return remotenames and tracked draft heads.
// - Get rid of "filtered revs" and "repo view" layer entirely.
// - Switch phases to be defined by heads (remotenames), instead of roots.

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "revlogindex"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<revlogindex>(py)?;
    Ok(m)
}

py_class!(class revlogindex |py| {
    data index: RefCell<RevlogIndex>;

    def __new__(_cls, changelogipath: String, nodemappath: String) -> PyResult<Self> {
        let changelogipath = Path::new(&changelogipath);
        let nodemappath = Path::new(&nodemappath);
        let index = RevlogIndex::new(&changelogipath, &nodemappath).map_pyerr(py)?;
        Self::create_instance(py, RefCell::new(index))
    }

    /// Obtain the index data.
    def indexdata(&self) -> PyResult<pybytes::Bytes> {
        let bytes = self.index(py).borrow().changelogi_data.clone();
        pybytes::Bytes::from_bytes(py, bytes)
    }

    /// Calculate `heads(ancestors(revs))`.
    def headsancestors(&self, revs: Vec<u32>) -> PyResult<Vec<u32>> {
        let revlog = self.index(py).borrow();
        Ok(revlog.headsancestors(revs).map_pyerr(py)?)
    }

    /// Given public and draft head revision numbers, calculate the "phase sets".
    /// Return (publicset, draftset).
    def phasesets(&self, publicheads: Vec<u32>, draftheads: Vec<u32>) -> PyResult<(Spans, Spans)> {
        let revlog = self.index(py).borrow();
        let (public_set, draft_set) = revlog.phasesets(publicheads, draftheads).map_pyerr(py)?;
        Ok((Spans(public_set), Spans(draft_set)))
    }

    /// Get parent revisions.
    def parentrevs(&self, rev: u32) -> PyResult<Vec<u32>> {
        let revlog = self.index(py).borrow();
        Ok(revlog.parent_revs(rev).map_pyerr(py)?.as_revs().to_vec())
    }

    /// Insert a new revision that hasn't been written to disk.
    /// Used by revlog._addrevision.
    def insert(&self, node: PyBytes, parents: Vec<u32>, data: Option<PyBytes> = None) -> PyResult<PyNone> {
        let node = node.data(py).to_vec().into();
        let mut revlog = self.index(py).borrow_mut();
        let data = data.map(|p| p.data(py).to_vec()).unwrap_or_default();
        revlog.insert(node, parents, data.into());
        Ok(PyNone)
    }

    def __len__(&self) -> PyResult<usize> {
        let revlog = self.index(py).borrow();
        Ok(revlog.len())
    }
});
