// Copyright Facebook, Inc. 2017
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![allow(non_camel_case_types)]

use cpython::py_module_initializer;

pub mod blackbox;
pub mod bookmarkstore;
pub mod configparser;
pub mod dag;
pub mod edenapi;
mod init;
pub mod lz4;
pub mod mutationstore;
pub mod nodemap;
pub mod pathmatcher;
pub mod revisionstore;
pub mod treestate;
pub mod vlq;
pub mod zstd;

py_module_initializer!(bindings, initbindings, PyInit_bindings, |py, m| {
    init::init_rust();
    let name = m.get(py, "__name__")?.extract::<String>(py)?;
    m.add(py, "__doc__", "Mercurial Rust Bindings")?;
    m.add(py, "blackbox", blackbox::init_module(py, &name)?)?;
    m.add(py, "bookmarkstore", bookmarkstore::init_module(py, &name)?)?;
    m.add(py, "configparser", configparser::init_module(py, &name)?)?;
    m.add(py, "dag", dag::init_module(py, &name)?)?;
    m.add(py, "edenapi", edenapi::init_module(py, &name)?)?;
    m.add(py, "lz4", lz4::init_module(py, &name)?)?;
    m.add(py, "mutationstore", mutationstore::init_module(py, &name)?)?;
    m.add(py, "nodemap", nodemap::init_module(py, &name)?)?;
    m.add(py, "pathmatcher", pathmatcher::init_module(py, &name)?)?;
    m.add(py, "revisionstore", revisionstore::init_module(py, &name)?)?;
    m.add(py, "treestate", treestate::init_module(py, &name)?)?;
    m.add(py, "vlq", vlq::init_module(py, &name)?)?;
    m.add(py, "zstd", zstd::init_module(py, &name)?)?;
    Ok(())
});
