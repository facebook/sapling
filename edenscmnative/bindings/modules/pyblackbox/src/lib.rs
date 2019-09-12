// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Python bindings for native blackbox logging.

use blackbox::{self, event::Event, init, log, serde_json, BlackboxOptions, ToValue};
use cpython::*;
use cpython_failure::ResultPyErrExt;
use encoding::local_bytes_to_path;
use std::ops::Deref;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "blackbox"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "init",
        py_fn!(
            py,
            init_blackbox(path: &PyBytes, count: u8 = 3, size: u64 = 100000000)
        ),
    )?;
    m.add(py, "_logjson", py_fn!(py, log_json(json: String)))?;
    m.add(py, "sync", py_fn!(py, sync()))?;
    m.add(
        py,
        "filter",
        py_fn!(py, filter(start: f64, end: f64, json: String)),
    )?;

    // _logjson takes a JSON string. Make it easier to use by
    // exposing a 'log' function that takes a Python object.
    // This is easier in Python than rust-cpython.
    let d = m.dict(py);
    d.set_item(py, "_json", py.import("json")?)?;
    py.run(
        r#"
def log(value, _dumps=_json.dumps, _logjson=_logjson):
    return _logjson(_dumps(value, ensure_ascii=0, check_circular=0))"#,
        Some(&d),
        None,
    )?;
    Ok(m)
}

/// Initialize the blackbox at the given path.
fn init_blackbox(py: Python, path: &PyBytes, count: u8, size: u64) -> PyResult<PyObject> {
    let path = local_bytes_to_path(path.data(py)).map_pyerr::<exc::RuntimeError>(py)?;
    let blackbox = BlackboxOptions::new()
        .max_bytes_per_log(size)
        .max_log_count(count)
        .open(path)
        .map_pyerr::<exc::IOError>(py)?;
    init(blackbox);
    Ok(py.None())
}

/// Log a JSON-serialized event. The JSON string must be deserializable
/// to the Rust Event type, defined in blackbox/src/event.rs.
fn log_json(py: Python, json: String) -> PyResult<PyObject> {
    let event = Event::from_json(&json).map_pyerr::<exc::RuntimeError>(py)?;
    log(&event);
    Ok(py.None())
}

/// Write buffered changes to disk.
fn sync(py: Python) -> PyResult<PyObject> {
    blackbox::sync();
    Ok(py.None())
}

/// Read events in the given time span. Return `[(session_id, timestamp, message, json)]`.
/// Timestamps are in seconds.
fn filter(
    py: Python,
    start: f64,
    end: f64,
    json: String,
) -> PyResult<Vec<(u64, f64, String, String)>> {
    if let Ok(blackbox) = blackbox::SINGLETON.lock() {
        let blackbox = blackbox.deref();
        // Blackbox uses millisecond integers. Translate seconds to milliseconds.
        let filter = blackbox::IndexFilter::Time((start * 1000.0) as u64, (end * 1000.0) as u64);
        let pattern = if json.is_empty() {
            None
        } else {
            Some(serde_json::from_str(&json).map_pyerr::<exc::RuntimeError>(py)?)
        };
        let events = blackbox.filter::<Event>(filter, pattern);
        return Ok(events
            .into_iter()
            .map(|e| {
                (
                    e.session_id,
                    // Translate back to float seconds.
                    (e.timestamp as f64) / 1000.0,
                    format!("{}", e.data),
                    // JSON formatted string.
                    serde_json::to_string(&e.data.to_value()).unwrap(),
                )
            })
            .collect());
    }

    Ok(Vec::new())
}
