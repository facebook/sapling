/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use cpython::*;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "drawdag"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "parse", py_fn!(py, parse(text: &str)))?;
    Ok(m)
}

fn parse(_py: Python, text: &str) -> PyResult<BTreeMap<String, Vec<String>>> {
    let parsed = drawdag::parse(text);
    let result = parsed
        .into_iter()
        .map(|(k, vs)| (k, vs.into_iter().collect()))
        .collect();
    Ok(result)
}
