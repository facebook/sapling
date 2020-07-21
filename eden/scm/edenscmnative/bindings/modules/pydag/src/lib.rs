/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use cpython::*;

pub mod commits;
pub mod dagalgo;
pub mod idmap;
pub mod nameset;
pub mod spanset;

pub use nameset::Names;
pub use spanset::spans;
pub use spanset::Spans;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "dag"].join(".");
    let m = PyModule::new(py, &name)?;

    // main entry type
    m.add_class::<commits::commits>(py)?;

    // "traits"
    m.add_class::<dagalgo::dagalgo>(py)?;
    m.add_class::<idmap::idmap>(py)?;

    // smartset-like types
    m.add_class::<nameset::nameset>(py)?;
    m.add_class::<spanset::spans>(py)?;

    // maximum Id
    m.add(py, "MAX_ID", dag::Id::MAX.0)?;

    Ok(m)
}
