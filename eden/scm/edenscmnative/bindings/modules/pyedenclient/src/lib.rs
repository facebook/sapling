/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "edenclient"].join(".");
    let m = PyModule::new(py, &name)?;
    #[cfg(feature = "eden")]
    m.add_class::<feature_eden::EdenFsClient>(py)?;
    Ok(m)
}

#[cfg(feature = "eden")]
mod feature_eden;
