/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::sync::Arc;

use configmodel::Config;
use configmodel::ConfigExt;
use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::ResultPyErrExt;
use cpython_ext::convert::ImplInto;
use dag::Repair;

pub mod commits;
pub mod dagalgo;
pub mod idmap;
mod impl_into;
mod parents;
pub mod set;
pub mod spanset;
mod verlink;

pub use set::Names;
pub use spanset::Spans;
pub use spanset::spans;
pub(crate) use verlink::VerLink;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "dag"].join(".");
    let m = PyModule::new(py, &name)?;

    // main entry type
    m.add_class::<commits::commits>(py)?;

    // "traits"
    m.add_class::<dagalgo::dagalgo>(py)?;
    m.add_class::<idmap::idmap>(py)?;
    m.add_class::<parents::parents>(py)?;

    // smartset-like types
    m.add_class::<set::nameset>(py)?;
    m.add_class::<spanset::spans>(py)?;

    // maximum Id
    m.add(py, "MAX_ID", dag::Id::MAX.0)?;

    // Explain bytes in indexedlog IdDag. For troubleshooting purpose.
    m.add(
        py,
        "describebytes",
        py_fn!(py, describe_bytes(bytes: PyBytes)),
    )?;

    // Repair Dag storage.
    m.add(py, "repair", py_fn!(py, repair(path: &PyPath)))?;

    // Configure related settings.
    m.add(
        py,
        "configure",
        py_fn!(py, configure(config: ImplInto<Arc<dyn Config + Send + Sync>>)),
    )?;

    impl_into::register(py);

    Ok(m)
}

fn describe_bytes(py: Python, bytes: PyBytes) -> PyResult<String> {
    let data = bytes.data(py);
    Ok(dag::describe_indexedlog_entry(data))
}

fn repair(py: Python, path: &PyPath) -> PyResult<String> {
    dag::Dag::repair(path).map_pyerr(py)
}

fn configure(py: Python, config: ImplInto<Arc<dyn Config + Send + Sync>>) -> PyResult<PyNone> {
    let config = config.into();
    let use_legacy_order = config
        .get_or_default::<bool>("experimental", "pydag-use-legacy-union-order")
        .map_pyerr(py)?;
    if use_legacy_order {
        set::USE_LEGACY_UNION_ORDER.store(true, std::sync::atomic::Ordering::Release);
    }
    Ok(PyNone)
}
