/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use cpython::*;
use cpython_ext::PyPath;
use cpython_ext::ResultPyErrExt;
use dag::Repair;

pub mod commits;
pub mod dagalgo;
pub mod idmap;
mod impl_into;
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

    // Explain bytes in indexedlog IdDag. For troubleshotting purpose.
    m.add(
        py,
        "describebytes",
        py_fn!(py, describe_bytes(bytes: PyBytes)),
    )?;

    // Repair NameDag storage.
    m.add(py, "repair", py_fn!(py, repair(path: &PyPath)))?;

    impl_into::register(py);

    Ok(m)
}

fn describe_bytes(py: Python, bytes: PyBytes) -> PyResult<String> {
    let data = bytes.data(py);
    Ok(dag::describe_indexedlog_entry(data))
}

fn repair(py: Python, path: &PyPath) -> PyResult<String> {
    dag::NameDag::repair(path).map_pyerr(py)
}
