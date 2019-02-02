// Copyright Facebook, Inc. 2017
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate bookmarkstore as rust_bookmarkstore;
extern crate byteorder;
extern crate configparser as rust_configparser;
#[macro_use]
extern crate cpython;
extern crate cpython_ext;
extern crate cpython_failure;
extern crate encoding;
extern crate failure;
extern crate lz4_pyframe;
extern crate mutationstore as rust_mutationstore;
extern crate nodemap as rust_nodemap;
extern crate pathmatcher as rust_pathmatcher;
extern crate treestate as rust_treestate;
extern crate types;
extern crate vlqencoding;
extern crate zstd as rust_zstd;
extern crate zstdelta as rust_zstdelta;

use cpython::py_module_initializer;

pub mod bookmarkstore;
pub mod configparser;
pub mod lz4;
pub mod mutationstore;
pub mod nodemap;
pub mod pathmatcher;
pub mod treestate;
pub mod zstd;

py_module_initializer!(bindings, initbindings, PyInit_bindings, |py, m| {
    let name = m.get(py, "__name__")?.extract::<String>(py)?;
    m.add(py, "__doc__", "Mercurial Rust Bindings")?;
    m.add(py, "bookmarkstore", bookmarkstore::init_module(py, &name)?)?;
    m.add(py, "configparser", configparser::init_module(py, &name)?)?;
    m.add(py, "lz4", lz4::init_module(py, &name)?)?;
    m.add(py, "mutationstore", mutationstore::init_module(py, &name)?)?;
    m.add(py, "nodemap", nodemap::init_module(py, &name)?)?;
    m.add(py, "pathmatcher", pathmatcher::init_module(py, &name)?)?;
    m.add(py, "treestate", treestate::init_module(py, &name)?)?;
    m.add(py, "zstd", zstd::init_module(py, &name)?)?;
    Ok(())
});
