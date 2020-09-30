/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Rust bindings for Mercurial's Python `progress` module.
//!
//! This crate provides wrappers around Mercurial's Python progress bar objects
//! so that they may be used by pure Rust code, as well as compatibility shims
//! so that Python code can also use the Rust progress API. This will enable
//! the eventual transition to a pure Rust progress bar implementation.

#![allow(non_camel_case_types)]

use cpython::*;

pub use rust::PyProgressFactory;

mod python;
mod rust;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "progress"].join(".");
    let m = PyModule::new(py, &name)?;

    m.add_class::<python::bar>(py)?;
    m.add_class::<python::spinner>(py)?;

    Ok(m)
}
