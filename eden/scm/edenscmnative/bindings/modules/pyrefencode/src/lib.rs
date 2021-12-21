/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use cpython::*;
use cpython_ext::convert::Serde;
use cpython_ext::ResultPyErrExt;
use refencode::HgId;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "refencode"].join(".");
    let m = PyModule::new(py, &name)?;

    m.add(
        py,
        "decodebookmarks",
        py_fn!(py, decodebookmarks(data: PyBytes)),
    )?;
    m.add(
        py,
        "decoderemotenames",
        py_fn!(py, decoderemotenames(data: PyBytes)),
    )?;
    m.add(
        py,
        "decodevisibleheads",
        py_fn!(py, decodevisibleheads(data: PyBytes)),
    )?;

    m.add(
        py,
        "encodebookmarks",
        py_fn!(
            py,
            encodebookmarks(namenodes: Serde<BTreeMap<String, HgId>>)
        ),
    )?;
    m.add(
        py,
        "encoderemotenames",
        py_fn!(
            py,
            encoderemotenames(namenodes: Serde<BTreeMap<String, HgId>>)
        ),
    )?;
    m.add(
        py,
        "encodevisibleheads",
        py_fn!(py, encodevisibleheads(namenodes: Serde<Vec<HgId>>)),
    )?;

    Ok(m)
}

fn decodebookmarks(py: Python, data: PyBytes) -> PyResult<Serde<BTreeMap<String, HgId>>> {
    let data = data.data(py);
    let decoded = refencode::decode_bookmarks(data).map_pyerr(py)?;
    Ok(Serde(decoded))
}

fn decoderemotenames(py: Python, data: PyBytes) -> PyResult<Serde<BTreeMap<String, HgId>>> {
    let data = data.data(py);
    let decoded = refencode::decode_remotenames(data).map_pyerr(py)?;
    Ok(Serde(decoded))
}

fn decodevisibleheads(py: Python, data: PyBytes) -> PyResult<Serde<Vec<HgId>>> {
    let data = data.data(py);
    let decoded = refencode::decode_visibleheads(data).map_pyerr(py)?;
    Ok(Serde(decoded))
}

fn encodebookmarks(py: Python, namenodes: Serde<BTreeMap<String, HgId>>) -> PyResult<PyBytes> {
    let encoded = refencode::encode_bookmarks(&namenodes.0);
    Ok(PyBytes::new(py, encoded.as_ref()))
}

fn encoderemotenames(py: Python, namenodes: Serde<BTreeMap<String, HgId>>) -> PyResult<PyBytes> {
    let encoded = refencode::encode_remotenames(&namenodes.0);
    Ok(PyBytes::new(py, encoded.as_ref()))
}

fn encodevisibleheads(py: Python, nodes: Serde<Vec<HgId>>) -> PyResult<PyBytes> {
    let encoded = refencode::encode_visibleheads(&nodes.0);
    Ok(PyBytes::new(py, encoded.as_ref()))
}
