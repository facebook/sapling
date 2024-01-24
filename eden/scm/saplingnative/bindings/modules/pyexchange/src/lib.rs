/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use configmodel::Config;
use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::convert::Serde;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::ExtractInner;
use cpython_ext::PyNone;
use pydag::commits::commits;
use pyedenapi::PyClient;
use pymetalog::metalog;
use types::HgId;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "exchange"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "clone",
        py_fn!(
            py,
            clone(
                config: ImplInto<Arc<dyn Config>>,
                edenapi: &PyClient,
                metalog: metalog,
                commits: &commits,
                bookmarks: Vec<String>,
            )
        ),
    )?;
    m.add(
        py,
        "fastpull",
        py_fn!(
            py,
            fast_pull(
                config: ImplInto<Arc<dyn Config>>,
                edenapi: &PyClient,
                commits: &commits,
                old: Serde<Vec<HgId>>,
                new: Serde<Vec<HgId>>,
            )
        ),
    )?;

    Ok(m)
}

fn clone(
    py: Python,
    config: ImplInto<Arc<dyn Config>>,
    edenapi: &PyClient,
    metalog: metalog,
    commits: &commits,
    bookmarks: Vec<String>,
) -> PyResult<PyNone> {
    let client = edenapi.extract_inner(py);
    let commits = commits.get_inner(py);
    let mut commits = commits.write();
    let meta = metalog.metalog_rwlock(py);
    let mut meta = meta.write();
    exchange::clone(&config.into(), client, &mut meta, &mut commits, bookmarks).map_pyerr(py)?;
    Ok(PyNone)
}

fn fast_pull(
    py: Python,
    config: ImplInto<Arc<dyn Config>>,
    edenapi: &PyClient,
    commits: &commits,
    common: Serde<Vec<HgId>>,
    missing: Serde<Vec<HgId>>,
) -> PyResult<(u64, u64)> {
    let client = edenapi.extract_inner(py);
    let commits = commits.get_inner(py);
    let mut commits = commits.write();
    exchange::fast_pull(&config.into(), client, &mut commits, common.0, missing.0).map_pyerr(py)
}
