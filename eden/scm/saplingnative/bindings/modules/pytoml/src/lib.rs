/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::convert::Serde;
use cpython_ext::ResultPyErrExt;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "toml"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "loads", py_fn!(py, parse(text: &str)))?;
    Ok(m)
}

fn parse(py: Python, text: &str) -> PyResult<Serde<toml::Value>> {
    let value: toml::Value = toml::from_str(text).map_pyerr(py)?;
    Ok(Serde(value))
}
