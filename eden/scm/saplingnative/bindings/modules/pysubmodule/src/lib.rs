/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use cpython::*;
use cpython_ext::convert::Serde;
use submodule::Submodule;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "submodule"].join(".");
    let m = PyModule::new(py, &name)?;

    m.add(
        py,
        "parse_gitmodules",
        py_fn!(py, parse_gitmodules(data: &PyBytes)),
    )?;

    Ok(m)
}

fn parse_gitmodules(py: Python, data: &PyBytes) -> PyResult<Serde<Vec<Submodule>>> {
    Ok(Serde(submodule::parse_gitmodules(data.data(py))))
}
