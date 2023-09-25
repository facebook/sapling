/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "version"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "VERSION", ::version::VERSION)?;
    m.add(py, "VERSION_HASH", ::version::VERSION_HASH)?;
    Ok(m)
}
