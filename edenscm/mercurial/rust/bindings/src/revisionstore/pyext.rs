// Copyright Facebook, Inc. 2018
//! Python bindings for a Rust hg store

use std::cell::{Ref, RefCell, RefMut};

use cpython::*;
use failure::format_err;

use crate::revisionstore::pythonutil::to_pyerr;

/// The cpython crates forces us to use a `RefCell` for mutation, `OptionalRefCell` wraps all the logic
/// of dealing with it.
struct OptionalRefCell<T> {
    inner: RefCell<Option<T>>,
}

impl<T> OptionalRefCell<T> {
    pub fn new(value: T) -> OptionalRefCell<T> {
        OptionalRefCell {
            inner: RefCell::new(Some(value)),
        }
    }

    /// Obtain a reference on the stored value. Will fail if the value was previously consumed.
    pub fn get_value(&self) -> Result<Ref<T>, failure::Error> {
        let b = self.inner.borrow();
        if b.as_ref().is_none() {
            Err(format_err!("OptionalRefCell is None."))
        } else {
            Ok(Ref::map(b, |o| o.as_ref().unwrap()))
        }
    }

    /// Obtain a mutable reference on the stored value. Will fail if the value was previously
    /// consumed.
    pub fn get_mut_value(&self) -> Result<RefMut<T>, failure::Error> {
        let borrow = self.inner.try_borrow_mut()?;
        if borrow.as_ref().is_none() {
            Err(format_err!("OptionalRefCell is None."))
        } else {
            Ok(RefMut::map(borrow, |o| o.as_mut().unwrap()))
        }
    }

    /// Consume the stored value and returns it. Will fail if the value was previously consumed.
    pub fn take_value(&self) -> Result<T, failure::Error> {
        let opt = self.inner.try_borrow_mut()?.take();
        opt.ok_or_else(|| format_err!("None"))
    }
}

/// Wrapper around `OptionalRefCell<T>` to convert from `Result<T>` to `PyResult<T>`
pub struct PyOptionalRefCell<T> {
    inner: OptionalRefCell<T>,
}

impl<T> PyOptionalRefCell<T> {
    pub fn new(value: T) -> PyOptionalRefCell<T> {
        PyOptionalRefCell {
            inner: OptionalRefCell::new(value),
        }
    }

    pub fn get_value(&self, py: Python) -> PyResult<Ref<T>> {
        self.inner.get_value().map_err(|e| to_pyerr(py, &e.into()))
    }

    pub fn get_mut_value(&self, py: Python) -> PyResult<RefMut<T>> {
        self.inner
            .get_mut_value()
            .map_err(|e| to_pyerr(py, &e.into()))
    }

    pub fn take_value(&self, py: Python) -> PyResult<T> {
        self.inner.take_value().map_err(|e| to_pyerr(py, &e.into()))
    }
}
