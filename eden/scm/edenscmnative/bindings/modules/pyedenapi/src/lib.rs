/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! pyedenapi - Python bindings for the Rust `edenapi` crate

#![deny(warnings)]
#![allow(non_camel_case_types)]

use cpython::*;

mod client;
mod pyext;
mod stats;
mod util;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "edenapi"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<client::client>(py)?;
    m.add_class::<stats::stats>(py)?;
    Ok(m)
}
