/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use python3_sys as ffi;

extern "C" {
    fn PyInit_bdiff() -> *mut ffi::PyObject;
    fn PyInit_mpatch() -> *mut ffi::PyObject;
    fn PyInit_osutil() -> *mut ffi::PyObject;
    fn PyInit_parsers() -> *mut ffi::PyObject;
    fn PyInit_bser() -> *mut ffi::PyObject;

    #[cfg(windows)]
    fn PyInit__curses() -> *mut ffi::PyObject;
    #[cfg(windows)]
    fn PyInit__curses_panel() -> *mut ffi::PyObject;

    fn traceprof_enable();
    fn traceprof_disable();
    fn traceprof_report_stderr();

    fn sapling_cext_evalframe_set_pass_through(enabled: u8);
    fn sapling_cext_evalframe_resolve_frame(frame_ptr: usize) -> *const u8;
}

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "cext"].join(".");
    let m = PyModule::new(py, &name)?;
    let (bdiff, mpatch, osutil, parsers, bser) = unsafe {
        (
            PyObject::from_borrowed_ptr(py, PyInit_bdiff()),
            PyObject::from_borrowed_ptr(py, PyInit_mpatch()),
            PyObject::from_borrowed_ptr(py, PyInit_osutil()),
            PyObject::from_borrowed_ptr(py, PyInit_parsers()),
            PyObject::from_borrowed_ptr(py, PyInit_bser()),
        )
    };
    m.add(py, "bdiff", bdiff)?;
    m.add(py, "mpatch", mpatch)?;
    m.add(py, "osutil", osutil)?;
    m.add(py, "parsers", parsers)?;
    m.add(py, "bser", bser)?;

    m.add(
        py,
        "evalframe_set_pass_through",
        py_fn!(py, evalframe_set_pass_through(enabled: bool = true)),
    )?;

    #[cfg(windows)]
    unsafe {
        let curses = PyObject::from_borrowed_ptr(py, PyInit__curses());
        let panel = PyObject::from_borrowed_ptr(py, PyInit__curses_panel());
        m.add(py, "_curses", curses)?;
        m.add(py, "_curses_panel", panel)?;
    }

    m.add_class::<TraceProf>(py)?;

    Ok(m)
}

py_class!(pub class TraceProf |py| {
    def __new__(_cls) -> PyResult<Self> {
        Self::create_instance(py)
    }

    def __enter__(&self) -> PyResult<Self> {
        unsafe { traceprof_enable() };
        Ok(self.clone_ref(py))
    }

    def __exit__(&self, _ty: Option<PyType>, _value: PyObject, _traceback: PyObject) -> PyResult<bool> {
        unsafe { traceprof_disable() };
        Ok(false) // Do not suppress exception
    }

    /// Report tracing result to stderr.
    def report(&self) -> PyResult<Option<u8>> {
        unsafe { traceprof_report_stderr() };
        Ok(None)
    }
});

fn evalframe_set_pass_through(_py: Python, enabled: bool) -> PyResult<Option<u8>> {
    unsafe {
        sapling_cext_evalframe_set_pass_through(enabled as u8);
        sapling_cext_evalframe_resolve_frame(0); // keep the C function alive
    };
    Ok(None)
}
