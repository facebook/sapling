/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! pyedenapi - Python bindings for the Rust `edenapi` crate

#![allow(non_camel_case_types)]

use cpython::*;

mod client;
mod pyext;
mod pytypes;
mod stats;
mod util;

pub use client::client as PyClient;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "edenapi"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<client::client>(py)?;
    m.add_class::<stats::stats>(py)?;
    Ok(m)
}
