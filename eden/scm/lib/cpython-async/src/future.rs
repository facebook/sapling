/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;

use cpython::*;
use cpython_ext::ResultPyErrExt;
use futures::future::BoxFuture;
use futures::future::Future;
use futures::future::FutureExt;

// Type to make Python able to reason about a Rust future.
//
// Unlike TStream, it does not support typed lossless Python -> Rust conversion.
py_class!(pub class future |py| {
    data inner: RefCell<Option<BoxFuture<'static, PyResult<PyObject>>>>;

    /// Resolve the future and return the resolved object.
    def wait(&self) -> PyResult<PyObject> {
        let mut inner = None;
        std::mem::swap(&mut inner, &mut self.inner(py).borrow_mut());

        match inner {
            Some(future) => py.allow_threads(|| async_runtime::block_on(future)),
            None => Err(PyErr::new::<exc::ValueError, _>(py, "future was already waited")),
        }
    }
});

impl future {
    /// Convert Rust Future to Python object.
    pub fn new<T, E, F>(py: Python, f: F) -> PyResult<Self>
    where
        T: ToPyObject,
        E: Into<anyhow::Error>,
        F: Future<Output = Result<T, E>> + Send + 'static,
    {
        let future = f.map(|v| {
            let gil = Python::acquire_gil();
            let py = gil.python();
            v.map_pyerr(py).map(|v| v.into_py_object(py).into_object())
        });
        Self::create_instance(py, RefCell::new(Some(Box::pin(future))))
    }
}
