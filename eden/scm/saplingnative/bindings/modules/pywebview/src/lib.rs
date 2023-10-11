/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::convert::Serde;
use cpython_ext::ResultPyErrExt;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "webview"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "open_isl",
        py_fn!(py, open_isl(
        options: Serde<webview_app::ISLSpawnOptions>
                )),
    )?;
    Ok(m)
}

fn open_isl(py: Python, options: Serde<webview_app::ISLSpawnOptions>) -> PyResult<PyNone> {
    let opts: webview_app::ISLSpawnOptions = options.0;
    webview_app::open_isl(opts).map_pyerr(py)?;
    Ok(PyNone)
}
