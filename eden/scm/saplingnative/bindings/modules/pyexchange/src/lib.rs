/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use configmodel::Config;
use cpython::*;
use cpython_ext::ExtractInner;
use cpython_ext::ExtractInnerRef;
use cpython_ext::PyNone;
use cpython_ext::PyPathBuf;
use cpython_ext::convert::ImplInto;
use cpython_ext::convert::Serde;
use cpython_ext::error::ResultPyErrExt;
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
                edenapi: &PyClient,
                commits: &commits,
                old: Serde<Vec<HgId>>,
                new: Serde<Vec<HgId>>,
            )
        ),
    )?;
    m.add(
        py,
        "streaming_clone",
        py_fn!(
            py,
            streaming_clone(
                client: &PyClient,
                store_path: PyPathBuf,
                tag: Option<String> = None
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
    edenapi: &PyClient,
    commits: &commits,
    common: Serde<Vec<HgId>>,
    missing: Serde<Vec<HgId>>,
) -> PyResult<(u64, u64)> {
    let client = edenapi.extract_inner(py);
    let commits = commits.get_inner(py);
    let mut commits = commits.write();
    exchange::fast_pull(client, &mut commits, common.0, missing.0).map_pyerr(py)
}

/// Perform streaming clone using the given EdenAPI client.
///
/// Writes the changelog data to the given store path.
/// Returns a dict with 'index_bytes_written' and 'data_bytes_written'.
fn streaming_clone(
    py: Python,
    client: &PyClient,
    store_path: PyPathBuf,
    tag: Option<String>,
) -> PyResult<Serde<clone::StreamingCloneResult>> {
    let api = client.extract_inner_ref(py).clone();
    let store_path = store_path.to_path_buf();
    let result = py
        .allow_threads(|| clone::streaming_clone_to_files(api.as_ref(), &store_path, tag))
        .map_pyerr(py)?;
    Ok(Serde(result))
}
