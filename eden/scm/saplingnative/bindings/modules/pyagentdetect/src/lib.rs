/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "agentdetect"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "is_agent", py_fn!(py, is_agent()))?;
    m.add(
        py,
        "is_agent_acknowledged",
        py_fn!(py, is_agent_acknowledged()),
    )?;
    Ok(m)
}

/// Check whether the current process is being driven by an AI coding agent.
fn is_agent(_py: Python) -> PyResult<bool> {
    Ok(agentdetect::is_agent())
}

/// Check whether the agent has acknowledged reading the guidelines.
fn is_agent_acknowledged(_py: Python) -> PyResult<bool> {
    Ok(agentdetect::is_agent_acknowledged())
}
