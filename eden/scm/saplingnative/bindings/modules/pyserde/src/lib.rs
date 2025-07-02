/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::ResultPyErrExt;
use cpython_ext::convert::Serde;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "serde"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "toml_loads", py_fn!(py, toml_loads(text: &str)))?;

    // `import json` can take 10ms. bindings' json has lower import time.
    m.add(py, "json_loads", py_fn!(py, json_loads(text: &str)))?;
    m.add(
        py,
        "json_dumps",
        py_fn!(py, json_dumps(value: Serde<serde_json::Value>, pretty: bool = false)),
    )?;

    m.add(py, "cbor_loads", py_fn!(py, cbor_loads(data: PyBytes)))?;
    m.add(
        py,
        "cbor_dumps",
        py_fn!(py, cbor_dumps(value: Serde<serde_cbor::Value>)),
    )?;

    m.add(
        py,
        "mincode_loads",
        py_fn!(py, mincode_loads(data: PyBytes)),
    )?;
    m.add(
        py,
        "mincode_dumps",
        py_fn!(py, mincode_dumps(value: Vec<i64>)),
    )?;

    Ok(m)
}

fn toml_loads(py: Python, text: &str) -> PyResult<Serde<toml::Value>> {
    let value: toml::Value = toml::from_str(text).map_pyerr(py)?;
    Ok(Serde(value))
}

fn json_loads(py: Python, text: &str) -> PyResult<Serde<serde_json::Value>> {
    let value: serde_json::Value = serde_json::from_str(text).map_pyerr(py)?;
    Ok(Serde(value))
}

fn json_dumps(py: Python, value: Serde<serde_json::Value>, pretty: bool) -> PyResult<String> {
    let json = match pretty {
        false => serde_json::to_string(&value.0),
        true => serde_json::to_string_pretty(&value.0),
    }
    .map_pyerr(py)?;
    Ok(json)
}

fn cbor_loads(py: Python, data: PyBytes) -> PyResult<Serde<serde_cbor::Value>> {
    let value: serde_cbor::Value = serde_cbor::from_slice(data.data(py)).map_pyerr(py)?;
    Ok(Serde(value))
}

fn cbor_dumps(py: Python, value: Serde<serde_cbor::Value>) -> PyResult<PyBytes> {
    let cbor = serde_cbor::to_vec(&value.0).map_pyerr(py)?;
    let bytes = PyBytes::new(py, &cbor);
    Ok(bytes)
}

fn mincode_loads(py: Python, data: PyBytes) -> PyResult<Vec<i64>> {
    let value = mincode::deserialize(data.data(py)).map_pyerr(py)?;
    Ok(value)
}

fn mincode_dumps(py: Python, value: Vec<i64>) -> PyResult<PyBytes> {
    let mincode = mincode::serialize(&value).map_pyerr(py)?;
    let bytes = PyBytes::new(py, &mincode);
    Ok(bytes)
}
