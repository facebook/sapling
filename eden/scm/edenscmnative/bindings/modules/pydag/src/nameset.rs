/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::dagalgo::dagalgo;
use crate::idmap::NULL_NODE;
use anyhow::Result;
use cpython::*;
use cpython_ext::{AnyhowResultExt, ResultPyErrExt};
use dag::nameset::hints::Flags;
use dag::{nameset::NameIter, Set, Vertex};
use std::cell::RefCell;
use std::collections::HashMap;

/// A wrapper around [`Set`] with Python integration added.
///
/// Differences from the `py_class` version:
/// - Auto converts from a wider range of Python types - not just nameset, but
///   also List[bytes], and Generator[bytes].
/// - Pure Rust. No need to take the Python GIL to create `Names`.
pub struct Names(pub Set);

// A wrapper around [`Set`].
py_class!(pub class nameset |py| {
    data inner: Set;

    def __new__(_cls, obj: PyObject) -> PyResult<Self> {
        Ok(Names::extract(py, &obj)?.to_py_object(py))
    }

    def __contains__(&self, name: PyBytes) -> PyResult<bool> {
        let name = Vertex::copy_from(name.data(py));
        Ok(self.inner(py).contains(&name).map_pyerr(py)?)
    }

    def __len__(&self) -> PyResult<usize> {
        Ok(self.inner(py).count().map_pyerr(py)?)
    }

    def __repr__(&self) -> PyResult<String> {
        Ok(format!("{:?}", self.inner(py)))
    }

    def __add__(lhs, rhs) -> PyResult<Names> {
        let lhs = Names::extract(py, lhs)?;
        let rhs = Names::extract(py, rhs)?;
        Ok(Names(lhs.0.union(&rhs.0)))
    }

    def __or__(lhs, rhs) -> PyResult<Names> {
        let lhs = Names::extract(py, lhs)?;
        let rhs = Names::extract(py, rhs)?;
        Ok(Names(lhs.0.union(&rhs.0)))
    }

    def __and__(lhs, rhs) -> PyResult<Names> {
        let lhs = Names::extract(py, lhs)?;
        let rhs = Names::extract(py, rhs)?;
        Ok(Names(lhs.0.intersection(&rhs.0)))
    }

    def __sub__(lhs, rhs) -> PyResult<Names> {
        let lhs = Names::extract(py, lhs)?;
        let rhs = Names::extract(py, rhs)?;
        Ok(Names(lhs.0.difference(&rhs.0)))
    }

    def __iter__(&self) -> PyResult<nameiter> {
        self.iter(py)
    }

    def iterrev(&self) -> PyResult<nameiter> {
        let iter = self.inner(py).clone().iter_rev().map_pyerr(py)?;
        let iter: RefCell<Box<dyn NameIter>> = RefCell::new(iter);
        nameiter::create_instance(py, iter)
    }

    def iter(&self) -> PyResult<nameiter> {
        let iter = self.inner(py).clone().iter().map_pyerr(py)?;
        let iter: RefCell<Box<dyn NameIter>> = RefCell::new(iter);
        nameiter::create_instance(py, iter)
    }

    def first(&self) -> PyResult<Option<PyBytes>> {
        Ok(self.inner(py).first().map_pyerr(py)?.map(|name| PyBytes::new(py, name.as_ref())))
    }

    def last(&self) -> PyResult<Option<PyBytes>> {
        Ok(self.inner(py).last().map_pyerr(py)?.map(|name| PyBytes::new(py, name.as_ref())))
    }

    /// Obtain an optional dag bound to this set.
    def dag(&self) -> PyResult<Option<dagalgo>> {
        match self.inner(py).dag() {
            Some(dag) => dagalgo::from_arc_dag(py, dag).map(Some),
            None => Ok(None),
        }
    }

    /// Convert the set to a plain static set.
    def flatten(&self) -> PyResult<Names> {
        let inner = self.inner(py);
        let set = inner.flatten().map_pyerr(py)?;
        Ok(Names(set))
    }

    def hints(&self) -> PyResult<HashMap<&'static str, PyObject>> {
        let mut result = HashMap::new();
        let hints = self.inner(py).hints();
        if let Some(id) = hints.min_id() {
            result.insert("min", id.0.to_py_object(py).into_object());
        }
        if let Some(id) = hints.max_id() {
            result.insert("max", id.0.to_py_object(py).into_object());
        }
        let flags = hints.flags();
        if flags.contains(Flags::ID_DESC) {
            result.insert("desc", py.True().into_object());
        }
        if flags.contains(Flags::ID_ASC) {
            result.insert("asc", py.True().into_object());
        }
        if flags.contains(Flags::TOPO_DESC) {
            result.insert("topo", py.True().into_object());
        }
        if flags.contains(Flags::EMPTY) {
            result.insert("empty", py.True().into_object());
        }
        if flags.contains(Flags::FULL) {
            result.insert("full", py.True().into_object());
        }
        if flags.contains(Flags::ANCESTORS) {
            result.insert("ancestors", py.True().into_object());
        }
        Ok(result)
    }
});

// A wrapper to [`NameIter`].
py_class!(pub class nameiter |py| {
    data iter: RefCell<Box<dyn NameIter>>;

    def __next__(&self) -> PyResult<Option<PyBytes>> {
        let mut iter = self.iter(py).borrow_mut();
        let next: Option<Vertex> = iter.next().transpose().map_pyerr(py)?;
        Ok(next.map(|name| PyBytes::new(py, name.as_ref())))
    }

    def __iter__(&self) -> PyResult<nameiter> {
        Ok(self.clone_ref(py))
    }
});

impl<'a> FromPyObject<'a> for Names {
    fn extract(py: Python, obj: &'a PyObject) -> PyResult<Self> {
        // type(obj) is nameset - convert to Names directly.
        if let Ok(pyset) = obj.extract::<nameset>(py) {
            return Ok(Names(pyset.inner(py).clone()));
        }

        // type(obj) is list - convert to StaticSet
        if let Ok(pylist) = obj.extract::<Vec<PyBytes>>(py) {
            let set = Set::from_static_names(pylist.into_iter().filter_map(|name| {
                let data = name.data(py);
                // Skip "nullid" automatically.
                if data == &NULL_NODE[..] {
                    None
                } else {
                    Some(Vertex::copy_from(data))
                }
            }));
            return Ok(Names(set));
        }

        // Others - convert to LazySet.
        let obj = obj.clone_ref(py);
        let iter = PyNameIter::new(py, obj.iter(py)?.into_object())?;
        let set = Set::from_iter(iter);
        Ok(Names(set))
    }
}

/// Similar to `PyIterator`, but without lifetime and has `Vertex` as
/// output type.
struct PyNameIter {
    obj: PyObject,
    errored: bool,
}

impl PyNameIter {
    fn new(py: Python, obj: PyObject) -> PyResult<Self> {
        let _obj = obj.iter(py)?;
        Ok(Self {
            obj,
            errored: false,
        })
    }
}

impl Iterator for PyNameIter {
    type Item = dag::Result<Vertex>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.errored {
            return None;
        }
        (|| -> PyResult<Option<Vertex>> {
            let gil = Python::acquire_gil();
            let py = gil.python();
            let mut iter = self.obj.iter(py)?;
            match iter.next() {
                None => Ok(None),
                Some(Ok(value)) => {
                    let value = value.extract::<PyBytes>(py)?;
                    let data = value.data(py);
                    if data == &NULL_NODE[..] {
                        // Skip "nullid" automatically.
                        self.next().transpose().map_pyerr(py)
                    } else {
                        Ok(Some(Vertex::copy_from(data)))
                    }
                }
                Some(Err(err)) => {
                    self.errored = true;
                    Err(err)
                }
            }
        })()
        .into_anyhow_result()
        .map_err(|e: anyhow::Error| dag::errors::BackendError::Other(e).into())
        .transpose()
    }
}

impl ToPyObject for Names {
    type ObjectType = nameset;

    fn to_py_object(&self, py: Python) -> Self::ObjectType {
        nameset::create_instance(py, self.0.clone()).unwrap()
    }
}
