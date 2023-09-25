/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cpython::*;
use cpython_ext::convert::Serde;
use cpython_ext::PyNone;
use cpython_ext::ResultPyErrExt;
use nodeipc::get_singleton;
use serde_json::Value;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "nodeipc"].join(".");
    let m = PyModule::new(py, &name)?;
    let ipc = NodeIpc::create_instance(py, Box::new(get_singleton))?;
    m.add(py, "IPC", ipc)?;
    Ok(m)
}

py_class!(class NodeIpc |py| {
    data inner: Box<dyn Fn() -> Option<Arc<nodeipc::NodeIpc>> + Send>;

    /// send(json_like) -> None
    ///
    /// Send a message. Might block.
    /// Do nothing if the other side is not connected.
    def send(&self, message: Serde<Value>) -> PyResult<PyNone> {
        let inner = (self.inner(py))();
        if let Some(ipc) = inner {
            let message = message.0;
            py.allow_threads(move || ipc.send(message)).map_pyerr(py)?
        }
        Ok(PyNone)
    }

    /// recv() -> Optional[json_like]
    ///
    /// Receive a message. Might block. Returns None if there are
    /// no more messages to receive.
    def recv(&self) -> PyResult<Option<Serde<Value>>> {
        let inner = (self.inner(py))();
        if let Some(ipc) = inner {
            let message = py
                .allow_threads(move || ipc.recv::<Value>())
                .map_pyerr(py)?;
            return Ok(message.map(Serde));
        }
        Ok(None)
    }

    // Test if the other side is connected.
    // No "///" docstring due to "///" makes this a regular function,
    // i.e. it has to be called with `ipc.__bool__`, not `bool(ipc)`.
    def __bool__(&self) -> PyResult<bool> {
        let inner = (self.inner(py))();
        Ok(inner.is_some())
    }
});
