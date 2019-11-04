/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use cpython::*;
use cpython_ext::Bytes;
use cpython_failure::ResultPyErrExt;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use tracing_collector::{
    model::{Action, EspanId},
    TracingData,
};

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "tracing"].join(".");
    let m = PyModule::new(py, &name)?;

    m.add_class::<tracingdata>(py)?;
    let singleton = tracingdata::create_instance(py, DATA.clone())?;
    m.add(py, "singleton", singleton)?;
    Ok(m)
}

lazy_static! {
    // This is public so other libraries can replace it.
    pub static ref DATA: Arc<Mutex<TracingData>> = Arc::new(Mutex::new(TracingData::new()));
}

py_class!(class tracingdata |py| {
    data data: Arc<Mutex<TracingData>>;

    def __new__(_cls) -> PyResult<tracingdata> {
        Self::create_instance(py, Arc::new(Mutex::new(tracing_collector::TracingData::new())))
    }

    @staticmethod
    def deserialize(bytes: PyBytes, binary: bool = true) -> PyResult<tracingdata> {
        let bytes = bytes.data(py);
        let data: TracingData = if binary {
            mincode::deserialize(bytes).map_pyerr::<exc::ValueError>(py)?
        } else {
            serde_json::from_slice(bytes).map_pyerr::<exc::ValueError>(py)?
        };
        Self::create_instance(py, Arc::new(Mutex::new(data)))
    }

    /// Serialize to bytes.
    def serialize(&self, binary: bool = true) -> PyResult<Bytes> {
        let data = self.data(py).lock();
        if binary {
            let bytes = mincode::serialize(data.deref()).map_pyerr::<exc::ValueError>(py)?;
            Ok(Bytes::from(bytes))
        } else {
            let json = serde_json::to_string(data.deref()).map_pyerr::<exc::ValueError>(py)?;
            Ok(Bytes::from(json))
        }
    }

    /// Add a span. Return SpanId.
    /// The Span is not entered automatically.
    def span(&self, metadata: Vec<(String, String)>) -> PyResult<u64> {
        let metadata: Vec<(&str, &str)> = metadata.iter().map(|(k, v)| (k.as_ref(), v.as_ref())).collect();
        let mut data = self.data(py).lock();
        Ok(data.add_espan(&metadata, None).0)
    }

    /// Edit fields of a previously added span.
    def edit(&self, id: u64,  metadata: Vec<(String, String)>) -> PyResult<PyObject> {
        let mut data = self.data(py).lock();
        data.edit_espan(EspanId(id), metadata);
        Ok(py.None())
    }

    /// Enter a span.
    def enter(&self, id: u64) -> PyResult<PyObject> {
        let mut data = self.data(py).lock();
        data.add_action(EspanId(id), Action::EnterSpan);
        Ok(py.None())
    }

    /// Exit a span.
    def exit(&self, id: u64) -> PyResult<PyObject> {
        let mut data = self.data(py).lock();
        data.add_action(EspanId(id), Action::ExitSpan);
        Ok(py.None())
    }

    /// Add an event.
    def event(&self, metadata: Vec<(String, String)>) -> PyResult<PyObject> {
        let metadata: Vec<(&str, &str)> = metadata.iter().map(|(k, v)| (k.as_ref(), v.as_ref())).collect();
        let mut data = self.data(py).lock();
        let id = data.add_espan(&metadata, None);
        data.add_action(id, Action::Event);
        Ok(py.None())
    }

    /// Export as Trace Event.
    def traceevent(&self) -> PyResult<PyObject> {
        let data = self.data(py).lock();
        let trace_event = data.trace_event(Default::default());
        cpython_ext::ser::to_object(py, &trace_event)
    }

    /// Export as Trace Event.
    def traceeventjson(&self) -> PyResult<Bytes> {
        let data = self.data(py).lock();
        let mut buf = Vec::new();
        data.write_trace_event_json(&mut buf, Default::default()).map_pyerr::<exc::ValueError>(py)?;
        Ok(Bytes::from(buf))
    }

    /// Export as ASCII.
    ///
    /// `minduration` specifies the minimal duration threshold in micro seconds.
    /// The default value is 10000 (10 milliseconds).
    def ascii(&self, minduration: u64 = 10000) -> PyResult<String> {
        let mut opts = tracing_collector::model::AsciiOptions::default();
        opts.min_duration_micros_to_hide = minduration;
        Ok(self.data(py).lock().ascii(&opts))
    }

    /// Export as TreeSpans.
    ///
    /// The return type is:
    ///
    /// {(pid, tid): {"start": micros, "duration": micros | null, "children": [index], **meta}}
    def treespans(&self) -> PyResult<PyObject> {
        let data = self.data(py).lock();
        let tree_spans = data.tree_spans();
        cpython_ext::ser::to_object(py, &tree_spans)
    }

    /// Swap with the global singleton.
    def __enter__(&self) -> PyResult<PyObject> {
        self.swap_with_singleton(py);
        Ok(py.None())
    }

    /// Swap (back) with the global singleton.
    def __exit__(&self, _ty: Option<PyType>, _value: PyObject, _traceback: PyObject) -> PyResult<bool> {
        self.swap_with_singleton(py);
        Ok(false) // Do not suppress exception
    }
});

impl tracingdata {
    /// Swap self with the global singleton.
    fn swap_with_singleton(&self, py: Python) {
        let mut self_data = self.data(py).lock();
        if let Some(mut global_data) = DATA.try_lock() {
            // in this case, self_data == global_data, no need to swap
            std::mem::swap(self_data.deref_mut(), global_data.deref_mut())
        }
    }
}
