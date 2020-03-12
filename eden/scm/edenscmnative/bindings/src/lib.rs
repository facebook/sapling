/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use cpython::{PyModule, PyResult, Python};
use cpython_ext::PyNone;

/// Populate an existing empty module so it contains utilities.
pub fn populate_module(py: Python<'_>, module: &PyModule) -> PyResult<PyNone> {
    let m = module;
    let name = m.get(py, "__name__")?.extract::<String>(py)?;
    m.add(py, "__doc__", "Mercurial Rust Bindings")?;
    m.add(py, "blackbox", pyblackbox::init_module(py, &name)?)?;
    m.add(
        py,
        "bookmarkstore",
        pybookmarkstore::init_module(py, &name)?,
    )?;
    m.add(py, "cliparser", pycliparser::init_module(py, &name)?)?;
    m.add(py, "configparser", pyconfigparser::init_module(py, &name)?)?;
    m.add(py, "dag", pydag::init_module(py, &name)?)?;
    m.add(py, "dirs", pydirs::init_module(py, &name)?)?;
    m.add(py, "edenapi", pyedenapi::init_module(py, &name)?)?;
    m.add(py, "error", pyerror::init_module(py, &name)?)?;
    m.add(py, "fs", pyfs::init_module(py, &name)?)?;
    m.add(py, "hgtime", pyhgtime::init_module(py, &name)?)?;
    m.add(py, "indexes", pyindexes::init_module(py, &name)?)?;
    m.add(py, "lz4", pylz4::init_module(py, &name)?)?;
    m.add(py, "manifest", pymanifest::init_module(py, &name)?)?;
    m.add(py, "metalog", pymetalog::init_module(py, &name)?)?;
    m.add(
        py,
        "mutationstore",
        pymutationstore::init_module(py, &name)?,
    )?;
    m.add(py, "nodemap", pynodemap::init_module(py, &name)?)?;
    m.add(py, "pathmatcher", pypathmatcher::init_module(py, &name)?)?;
    m.add(py, "renderdag", pyrenderdag::init_module(py, &name)?)?;
    m.add(
        py,
        "revisionstore",
        pyrevisionstore::init_module(py, &name)?,
    )?;
    m.add(py, "revlogindex", pyrevlogindex::init_module(py, &name)?)?;
    m.add(py, "stackdesc", pystackdesc::init_module(py, &name)?)?;
    m.add(py, "threading", pythreading::init_module(py, &name)?)?;
    m.add(py, "tracing", pytracing::init_module(py, &name)?)?;
    m.add(py, "treestate", pytreestate::init_module(py, &name)?)?;
    m.add(py, "vlq", pyvlq::init_module(py, &name)?)?;
    m.add(py, "workingcopy", pyworkingcopy::init_module(py, &name)?)?;
    m.add(py, "worker", pyworker::init_module(py, &name)?)?;
    m.add(py, "zstd", pyzstd::init_module(py, &name)?)?;
    m.add(py, "zstore", pyzstore::init_module(py, &name)?)?;
    Ok(PyNone)
}
