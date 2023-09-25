/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::ResultPyErrExt;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "webview"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "open", py_fn!(py, open(url: &str, width: i32 = 800, height: i32 = 600, browser: Option<String> = None)))?;
    Ok(m)
}

fn open(
    py: Python,
    url: &str,
    width: i32,
    height: i32,
    browser: Option<String>,
) -> PyResult<PyNone> {
    webview_app::open(url, width, height, browser).map_pyerr(py)?;
    Ok(PyNone)
}
