/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;

py_exception!(error, EdenError);

pub(crate) fn populate_module(py: Python, m: &PyModule) -> PyResult<()> {
    m.add(py, "EdenError", py.get_type::<EdenError>())?;
    cpython_ext::error::register("020-eden-error", eden_error_handler);
    Ok(())
}

fn eden_error_handler(py: Python, mut e: &cpython_ext::error::Error) -> Option<PyErr> {
    // Remove anyhow contex.
    while let Some(inner) = e.downcast_ref::<cpython_ext::error::Error>() {
        e = inner;
    }

    if let Some(e) = e.downcast_ref::<edenfs_client::EdenError>() {
        return Some(PyErr::new::<EdenError, _>(py, &e.message));
    }

    None
}
