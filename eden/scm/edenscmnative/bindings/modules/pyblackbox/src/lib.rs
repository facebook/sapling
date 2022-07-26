/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Python bindings for native blackbox logging.

use std::ops::Deref;

use blackbox::event::Event;
use blackbox::init;
use blackbox::serde_json;
use blackbox::BlackboxOptions;
use blackbox::SessionId;
use blackbox::ToValue;
use cpython::*;
use cpython_ext::de::from_object;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::ResultPyErrExt;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "blackbox"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "init",
        py_fn!(
            py,
            init_blackbox(path: &PyPath, count: u8 = 3, size: u64 = 100000000)
        ),
    )?;
    m.add(py, "log", py_fn!(py, log(obj: PyObject)))?;
    m.add(py, "sync", py_fn!(py, sync()))?;
    m.add(
        py,
        "sessions",
        py_fn!(py, session_ids_by_pattern(obj: PyObject)),
    )?;
    m.add(
        py,
        "events",
        py_fn!(
            py,
            events_by_session_ids(session_ids: Vec<u64>, pattern: PyObject)
        ),
    )?;

    Ok(m)
}

/// Initialize the blackbox at the given path.
fn init_blackbox(py: Python, path: &PyPath, count: u8, size: u64) -> PyResult<PyNone> {
    let blackbox = BlackboxOptions::new()
        .max_bytes_per_log(size)
        .max_log_count(count)
        .open(path)
        .map_pyerr(py)?;
    init(blackbox);
    Ok(PyNone)
}

/// Log an event. The `obj` must be deserializable to the Rust Event type,
/// defined in `blackbox/src/event.rs`.
fn log(py: Python, obj: PyObject) -> PyResult<PyNone> {
    let event: Event = from_object(py, obj).map_pyerr(py)?;
    blackbox::log(&event);
    Ok(PyNone)
}

/// Write buffered changes to disk.
fn sync(_py: Python) -> PyResult<PyNone> {
    blackbox::sync();
    Ok(PyNone)
}

/// Read events in the given time span. Return `[(session_id, timestamp, message, json)]`.
/// Timestamps are in seconds.
fn session_ids_by_pattern(py: Python, obj: PyObject) -> PyResult<Vec<u64>> {
    let pattern: serde_json::Value = from_object(py, obj).map_pyerr(py)?;
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
    pattern: PyObject,
) -> PyResult<Vec<(u64, f64, String, String)>> {
    let pattern: serde_json::Value = from_object(py, pattern).map_pyerr(py)?;
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
