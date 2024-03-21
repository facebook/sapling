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
    feature_eden::populate_module(py, &m)?;
    Ok(m)
}

#[cfg(feature = "eden")]
pub mod feature_eden;
