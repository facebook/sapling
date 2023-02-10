/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::PyNone;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "atexit"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<AtExit>(py)?;
    m.add(py, "drop_queued", py_fn!(py, drop_queued()))?;
    Ok(m)
}

py_class!(pub class AtExit |py| {
    // We cannot use `AtExit` here, since it won't be a Rust variable
    // on stack or thread_local, and won't get dropped by Rust's
    // `std::process::exit`.
    data inner: atexit::AtExitRef;

    // We cannot take a general Python function as `drop`, since we might not be
    // able to obtain Python GIL to run it on drop without blocking.
    // Therefore, let's just add APIs to create specific `AtExit`s.

    /// Cancel the `AtExit` - do nothing at exit.
    def cancel(&self) -> PyResult<PyNone> {
        self.inner(py).cancel();
        Ok(PyNone)
    }

    /// rmtree(path) -> AtExit.
    /// Creates `AtExit` that deletes the given path at exit.
    @staticmethod
    def rmtree(path: String) -> PyResult<Self> {
        let func = Box::new(move || {
            if std::fs::remove_dir_all(&path).is_err() {
                let _ = std::fs::remove_file(&path);
            }
        });
        Self::new(py, func)
    }

    def __enter__(&self) -> PyResult<Self> {
        Ok(self.clone_ref(py))
    }

    def __exit__(&self, _ty: Option<PyType>, exc: PyObject, _tb: PyObject) -> PyResult<bool> {
        // Only cancel if there are no exceptions
        if exc.is_none(py) {
            self.inner(py).cancel();
        }
        // Do not suppress exception
        Ok(false)
    }
});

impl AtExit {
    fn new(py: Python, func: Box<dyn FnOnce() + Send + Sync + 'static>) -> PyResult<Self> {
        let inner = atexit::AtExit::new(func);
        let inner = inner.queued();
        Self::create_instance(py, inner)
    }
}

fn drop_queued(_py: Python) -> PyResult<PyNone> {
    atexit::drop_queued();
    Ok(PyNone)
}
