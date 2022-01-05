/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::collections::hash_map::Keys;
use std::collections::HashMap;
use std::str;

use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "dirs"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<dirs>(py)?;
    Ok(m)
}

fn add_path(map: &mut HashMap<PyPathBuf, u64>, path: &PyPath) {
    let path = path.as_str();
    for (i, _) in path.rmatch_indices("/").chain(Some((0, ""))) {
        let prefix = PyPath::from_str(&path[..i]);
        if let Some(e) = map.get_mut(prefix) {
            *e += 1;
            return;
        }
        map.insert(prefix.to_owned(), 1);
    }
}

fn del_path(py: Python, map: &mut HashMap<PyPathBuf, u64>, path: &PyPath) -> PyResult<()> {
    let path = path.as_str();
    for (i, _) in path.rmatch_indices("/").chain(Some((0, ""))) {
        let prefix = PyPath::from_str(&path[..i]);
        if let Some(e) = map.get_mut(prefix) {
            if *e > 1 {
                *e -= 1;
                return Ok(());
            }
        } else {
            return Err(PyErr::new::<exc::ValueError, _>(
                py,
                "path not in collection",
            ));
        }
        map.remove(prefix);
    }
    Ok(())
}

// A multi-set of the directories that contain paths.
py_class!(pub class dirs |py| {
    @shared data inner: HashMap<PyPathBuf, u64>;

    def __new__(_cls, init: Option<&PyObject>) -> PyResult<dirs> {
        let mut inner = HashMap::new();
        if let Some(init) = init {
            for path in init.iter(py)? {
                RefFromPyObject::with_extracted(py, &path?, |path: &PyPath| {
                    add_path(&mut inner, path);
                })?;
            }
        }
        Ok(dirs::create_instance(py, inner)?)
    }

    def addpath(&self, path: &PyPath) -> PyResult<PyNone> {
        let mut inner = self.inner(py).borrow_mut();
        add_path(&mut *inner, path);
        Ok(PyNone)
    }

    def delpath(&self, path: &PyPath) -> PyResult<PyNone> {
        let mut inner = self.inner(py).borrow_mut();
        del_path(py, &mut *inner, path)?;
        Ok(PyNone)
    }

    def __contains__(&self, path: PyPathBuf) -> PyResult<bool> {
        let inner = self.inner(py).borrow();
        Ok(inner.contains_key(&path))
    }

    def __len__(&self) -> PyResult<usize> {
        Ok(self.inner(py).borrow().len())
    }

    def __iter__(&self) -> PyResult<dirsiter> {
        let iter = self.inner(py).leak_immutable();
        dirsiter::create_instance(py, RefCell::new(unsafe { iter.map(py, |o| o.keys()) }))
    }
});

py_class!(pub class dirsiter |py| {
    data iter: RefCell<UnsafePyLeaked<Keys<'static, PyPathBuf, u64>>>;

    def __next__(&self) -> PyResult<Option<PyPathBuf>> {
        let mut iter = self.iter(py).borrow_mut();
        let mut iter = unsafe { iter.try_borrow_mut(py)? };
        Ok(iter.next().cloned())
    }

    def __iter__(&self) -> PyResult<dirsiter> {
        Ok(self.clone_ref(py))
    }
});
