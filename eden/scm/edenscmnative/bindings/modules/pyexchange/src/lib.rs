/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::ExtractInner;
use cpython_ext::PyNone;
use pyconfigparser::config;
use pydag::commits::commits;
use pyedenapi::PyClient;
use pymetalog::metalog;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "pull"].join(".");
    let m = PyModule::new(py, &name)?;
    // need to take in a repo
    m.add(
        py,
        "clone",
        py_fn!(
            py,
            clone(
                config: &config,
                edenapi: &PyClient,
                metalog: metalog,
                commits: &commits
            )
        ),
    )?;

    Ok(m)
}

fn clone(
    py: Python,
    config: &config,
    edenapi: &PyClient,
    metalog: metalog,
    commits: &commits,
) -> PyResult<PyNone> {
    let config = config.get_cfg(py);
    let client = edenapi.extract_inner(py);
    let commits = commits.get_inner(py);
    let mut commits = commits.write();
    let meta = metalog.metalog_rwlock(py);
    let mut meta = meta.write();
    exchange::clone(&config, client, &mut meta, &mut commits).map_pyerr(py)?;
    Ok(PyNone)
}
