/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::PyModule;
use cpython::PyResult;
use cpython::Python;
use cpython_ext::PyNone;

/// Populate an existing empty module so it contains utilities.
pub(crate) fn populate_module(py: Python<'_>, module: &PyModule) -> PyResult<PyNone> {
    let m = module;
    let name = m.get(py, "__name__")?.extract::<String>(py)?;
    m.add(py, "__doc__", "Mercurial Rust Bindings")?;
    m.add(py, "auth", pyauth::init_module(py, &name)?)?;
    m.add(py, "cats", pycats::init_module(py, &name)?)?;
    m.add(py, "blackbox", pyblackbox::init_module(py, &name)?)?;
    m.add(py, "bytes", pybytes::init_module(py, &name)?)?;
    m.add(py, "checkout", pycheckout::init_module(py, &name)?)?;
    m.add(py, "cliparser", pycliparser::init_module(py, &name)?)?;
    m.add(py, "configparser", pyconfigparser::init_module(py, &name)?)?;
    m.add(py, "dag", pydag::init_module(py, &name)?)?;
    m.add(py, "diffhelpers", pydiffhelpers::init_module(py, &name)?)?;
    m.add(py, "dirs", pydirs::init_module(py, &name)?)?;
    m.add(py, "doctor", pydoctor::init_module(py, &name)?)?;
    m.add(py, "drawdag", pydrawdag::init_module(py, &name)?)?;
    m.add(py, "eagerepo", pyeagerepo::init_module(py, &name)?)?;
    m.add(py, "edenapi", pyedenapi::init_module(py, &name)?)?;
    m.add(py, "error", pyerror::init_module(py, &name)?)?;
    m.add(py, "fail", pyfail::init_module(py, &name)?)?;
    m.add(py, "fs", pyfs::init_module(py, &name)?)?;
    m.add(py, "gitstore", pygitstore::init_module(py, &name)?)?;
    m.add(py, "hgmetrics", pyhgmetrics::init_module(py, &name)?)?;
    m.add(py, "hgtime", pyhgtime::init_module(py, &name)?)?;
    m.add(py, "indexes", pyindexes::init_module(py, &name)?)?;
    m.add(py, "lock", pylock::init_module(py, &name)?)?;
    m.add(py, "lz4", pylz4::init_module(py, &name)?)?;
    m.add(py, "manifest", pymanifest::init_module(py, &name)?)?;
    m.add(py, "metalog", pymetalog::init_module(py, &name)?)?;
    m.add(
        py,
        "mutationstore",
        pymutationstore::init_module(py, &name)?,
    )?;
    m.add(py, "nodemap", pynodemap::init_module(py, &name)?)?;
    m.add(py, "io", pyio::init_module(py, &name)?)?;
    m.add(py, "pathhistory", pypathhistory::init_module(py, &name)?)?;
    m.add(py, "pathmatcher", pypathmatcher::init_module(py, &name)?)?;
    m.add(py, "pprint", pypprint::init_module(py, &name)?)?;
    m.add(py, "process", pyprocess::init_module(py, &name)?)?;
    m.add(py, "progress", pyprogress::init_module(py, &name)?)?;
    m.add(py, "refencode", pyrefencode::init_module(py, &name)?)?;
    m.add(py, "regex", pyregex::init_module(py, &name)?)?;
    m.add(py, "renderdag", pyrenderdag::init_module(py, &name)?)?;
    m.add(py, "repo", pyrepo::init_module(py, &name)?)?;
    m.add(
        py,
        "revisionstore",
        pyrevisionstore::init_module(py, &name)?,
    )?;
    m.add(py, "revlogindex", pyrevlogindex::init_module(py, &name)?)?;
    m.add(py, "sptui", pysptui::init_module(py, &name)?)?;
    m.add(py, "status", pystatus::init_module(py, &name)?)?;
    m.add(py, "threading", pythreading::init_module(py, &name)?)?;
    m.add(py, "tracing", pytracing::init_module(py, &name)?)?;
    m.add(py, "treestate", pytreestate::init_module(py, &name)?)?;
    m.add(py, "vlq", pyvlq::init_module(py, &name)?)?;
    m.add(py, "workingcopy", pyworkingcopy::init_module(py, &name)?)?;
    m.add(py, "worker", pyworker::init_module(py, &name)?)?;
    m.add(py, "zstd", pyzstd::init_module(py, &name)?)?;
    m.add(py, "clientinfo", pyclientinfo::init_module(py, &name)?)?;
    m.add(py, "zstore", pyzstore::init_module(py, &name)?)?;

    Ok(PyNone)
}
