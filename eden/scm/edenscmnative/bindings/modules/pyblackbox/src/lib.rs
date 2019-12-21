/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Python bindings for native blackbox logging.

use blackbox::{self, event::Event, init, log, serde_json, BlackboxOptions, SessionId, ToValue};
use cpython::*;
use cpython_ext::ResultPyErrExt;
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
        "sessions",
        py_fn!(py, session_ids_by_pattern(json: &str)),
    )?;
    m.add(
        py,
        "events",
        py_fn!(
            py,
            events_by_session_ids(session_ids: Vec<u64>, pattern: &str)
        ),
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
    let path = local_bytes_to_path(path.data(py)).map_pyerr(py)?;
    let blackbox = BlackboxOptions::new()
        .max_bytes_per_log(size)
        .max_log_count(count)
        .open(path)
        .map_pyerr(py)?;
    init(blackbox);
    Ok(py.None())
}

/// Log a JSON-serialized event. The JSON string must be deserializable
/// to the Rust Event type, defined in blackbox/src/event.rs.
fn log_json(py: Python, json: String) -> PyResult<PyObject> {
    let event = Event::from_json(&json).map_pyerr(py)?;
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
fn session_ids_by_pattern(py: Python, pattern: &str) -> PyResult<Vec<u64>> {
    let pattern: serde_json::Value = serde_json::from_str(pattern).map_pyerr(py)?;
    let blackbox = blackbox::SINGLETON.lock();
    let blackbox = blackbox.deref();
    Ok(blackbox
        .session_ids_by_pattern(&pattern)
        .into_iter()
        .map(|id| id.0)
        .collect())
}

/// Read events with the given session ids.
/// Return `[(session_id, timestamp, message, json)]`.
fn events_by_session_ids(
    py: Python,
    session_ids: Vec<u64>,
    pattern: &str,
) -> PyResult<Vec<(u64, f64, String, String)>> {
    let pattern: serde_json::Value = serde_json::from_str(pattern).map_pyerr(py)?;
    let blackbox = blackbox::SINGLETON.lock();
    let blackbox = blackbox.deref();
    let mut result = Vec::new();
    for session_id in session_ids {
        for entry in blackbox.entries_by_session_id(SessionId(session_id)) {
            if !entry.match_pattern(&pattern) {
                continue;
            }
            let json = match &entry.data {
                // Skip converting TracingData to JSON.
                &Event::TracingData { serialized: _ } => "{}".to_string(),
                _ => serde_json::to_string(&entry.data.to_value()).unwrap(),
            };

            result.push((
                entry.session_id,
                // Translate back to float seconds.
                (entry.timestamp as f64) / 1000.0,
                format!("{}", entry.data),
                json,
            ));
        }
    }
    Ok(result)
}
