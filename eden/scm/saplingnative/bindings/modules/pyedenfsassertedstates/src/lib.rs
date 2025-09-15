/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! pyedenfsassertedstates - Python bindings for the Rust `edenfs-asserted-states` crate

#![allow(non_camel_case_types)]

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use configmodel::convert::FromConfigValue;
use cpython::*;
use cpython_ext::ResultPyErrExt;

#[cfg_attr(not(feature = "fb"), path = "dummy_fb.rs")]
mod fb;

use fb::AssertedStatesClient as RustAssertedStatesClient;
use fb::ContentLockGuard as RustContentLockGuard;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "edenfsassertedstates"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<AssertedStatesClient>(py)?;
    Ok(m)
}

py_class!(pub class AssertedStatesClient |py| {
    data inner: Arc<RustAssertedStatesClient>;

    def __new__(_cls, path: String) -> PyResult<AssertedStatesClient> {
        let rust_path = PathBuf::from(path);
        let inner = py.allow_threads(|| RustAssertedStatesClient::new(&rust_path)).map_pyerr(py)?;
        Self::create_instance(py, inner.into())
    }

    def enter_state_with_deadline(&self, state: &str, deadline: &str, backoff: &str) -> PyResult<PyContentLockGuard> {
        let inner = self.inner(py);
        let deadline_duration: Duration = FromConfigValue::try_from_str(deadline).map_pyerr(py)?;
        let backoff_duration: Duration = FromConfigValue::try_from_str(backoff).map_pyerr(py)?;
        let result = py.allow_threads(|| inner.enter_state_with_deadline(state,
            deadline_duration,
            backoff_duration
        )).map_pyerr(py)?;
        PyContentLockGuard::create_instance(py, Cell::new(result.into()))
    }
});

// Wrapper for ContentLockGuard + scoped semantics for python
py_class!(pub class PyContentLockGuard |py| {
    data lock: Cell<Option<RustContentLockGuard>>;

    def __enter__(&self) -> PyResult<Self> {
        Ok(self.clone_ref(py))
    }

    def __exit__(&self, _ty: Option<PyType>, _value: PyObject, _traceback: PyObject) -> PyResult<bool> {
        if let Some(f) = self.lock(py).replace(None) {
            drop(f);
            return Ok(false); // preserves exceptions
        } else {
            return Err(PyErr::new::<exc::ValueError, _>(py, "lock is already unlocked"));
        }
    }
});
