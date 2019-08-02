// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cpython::*;

use stackdesc::{render_stack, ScopeDescription};

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "stackdesc"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "renderstack", py_fn!(py, renderstack_py()))?;
    m.add(
        py,
        "describecall",
        py_fn!(py, describecall_py(desc: PyObject, body: PyObject)),
    )?;
    Ok(m)
}

// Call another function with stackdesc information attached to it.
//
// desc: how to get the longer description for the scope.
// body: defines the scope, function that will be called immediately.
//
// This is the basic building block
pub fn describecall_py(py: Python, desc: PyObject, body: PyObject) -> PyResult<PyObject> {
    let desc_func = || match desc.call(py, NoArgs, None) {
        Err(err) => render_error(py, err),
        Ok(desc) => match desc.extract::<String>(py) {
            Ok(desc) => desc,
            Err(err) => render_error(py, err),
        },
    };
    // ScopeDescription must be "first create, first drop". It's safer to
    // achieve that by making sure ScopeDescription is only a stack variable.
    // Python's contextmanager is conceptually nice, but has crticial flaws
    // (see D10850512).
    // Python's "stack variable" is also a bad idea because nothing prevents
    // it from being passed to elsewhere.
    let _scoped = ScopeDescription::new(desc_func);
    body.call(py, NoArgs, None)
}

pub fn renderstack_py(_py: Python) -> PyResult<Vec<(String)>> {
    Ok(render_stack())
}

fn render_error(py: Python, mut error: PyErr) -> String {
    let obj = error.instance(py);
    match obj.repr(py) {
        Ok(obj) => format!("<error {}>", obj.to_string_lossy(py)),
        Err(_) => "<error>".to_string(),
    }
}
