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
use pydag::commits::commits;
use pyedenapi::PyClient;
use pymetalog::metalog;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "pull"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "clone",
        py_fn!(
            py,
            clone(
                edenapi: &PyClient,
                metalog: metalog,
                commits: &commits,
                bookmarks: Vec<String>,
            )
        ),
    )?;

    Ok(m)
}

fn clone(
    py: Python,
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
    exchange::clone(client, &mut meta, &mut commits, bookmarks).map_pyerr(py)?;

    Ok(PyNone)
}
