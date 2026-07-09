/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use cpython::*;
use cpython_ext::ResultPyErrExt;
use filewalk::FileStats;
use filewalk::WalkInput;
use filewalk::WalkOptions;
use filewalk::prefetch;
use manifest_tree::TreeManifest;
use pyconfigloader::config as PyConfig;
use pymanifest::treemanifest as PyTreeManifest;
use pypathmatcher::extract_matcher;
use pyrepo::repo as PyRepo;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "filewalk"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "walkandcache",
        py_fn!(
            py,
            walk_and_cache_py(
                repo: PyRepo,
                manifests: &PyList,
                matcher: PyObject,
                config: PyConfig,
                base_manifest: Option<PyTreeManifest> = None,
            )
        ),
    )?;
    Ok(m)
}

fn walk_and_cache_py(
    py: Python,
    repo: PyRepo,
    manifests: &PyList,
    matcher: PyObject,
    config: PyConfig,
    base_manifest: Option<PyTreeManifest>,
) -> PyResult<PyDict> {
    let manifests = extract_manifests(py, manifests)?;
    let base_manifest = base_manifest.map(|manifest| manifest.get_underlying(py).read().clone());
    let matcher = extract_matcher(py, matcher)?.0;
    let options = WalkOptions::from_config(&config.get_cfg(py)).map_pyerr(py)?;

    let file_store = repo.read_repo(py).file_store().map_pyerr(py)?;

    let stats = py
        .allow_threads(move || {
            let mut stats = FileStats::default();
            for manifest in manifests {
                let input = match base_manifest.as_ref() {
                    Some(base_manifest) => WalkInput::Diff {
                        manifest,
                        base_manifest: base_manifest.clone(),
                    },
                    None => WalkInput::Manifest(manifest),
                };
                let fetch_stats = prefetch(input, matcher.clone(), &file_store, options)?;
                stats.local_files += fetch_stats.local_files;
                stats.remote_files += fetch_stats.remote_files;
            }
            anyhow::Ok(stats)
        })
        .map_pyerr(py)?;

    let py_stats = PyDict::new(py);
    py_stats.set_item(py, "local", stats.local_files)?;
    py_stats.set_item(py, "remote", stats.remote_files)?;
    Ok(py_stats)
}

fn extract_manifests(py: Python, manifests: &PyList) -> PyResult<Vec<TreeManifest>> {
    manifests
        .iter(py)
        .map(|manifest| {
            let manifest = PyTreeManifest::downcast_from(py, manifest)?;
            Ok(manifest.get_underlying(py).read().clone())
        })
        .collect()
}
