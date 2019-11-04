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
use std::cell::Cell;
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
    m.add_class::<meta>(py)?;
    m.add_class::<wrapfunc>(py)?;
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

fn getattr(py: Python, obj: &PyObject, name: &str) -> PyObject {
    obj.getattr(py, name).unwrap_or_else(|_| py.None())
}

fn tostr(py: Python, obj: PyObject) -> String {
    obj.str(py)
        .map(|s| s.to_string_lossy(py).to_string())
        .unwrap_or_else(|_| "<missing>".to_string())
}

// Decorator to set "meta" attribute.
py_class!(class meta |py| {
    data meta_obj: PyObject;

    def __new__(_cls, *args, **kwargs) -> PyResult<meta> {
        if (args.len(py) == 1 && kwargs.is_none()) || (args.len(py) == 0 && kwargs.is_some()) {
            let meta = match kwargs {
                Some(kwargs) => kwargs.items_list(py).into_object(),
                None => args.get_item(py, 0),
            };
            Self::create_instance(py, meta)
        } else {
            Err(PyErr::new::<exc::TypeError, _>(py, "invalid meta arguments"))
        }
    }

    def __call__(&self, obj: PyObject) -> PyResult<PyObject> {
        obj.setattr(py, "meta", self.meta_obj(py))?;
        Ok(obj)
    }
});

py_class!(class wrapfunc |py| {
    data inner: PyObject;
    data name: String;
    data module: String;
    data lineno: String;
    data meta: Option<PyObject>;
    data is_generator: bool;
    data last_espan_id: Cell<EspanId>;

    def __new__(_cls, obj: PyObject, meta: Option<PyObject> = None, classname: Option<String> = None) -> PyResult<PyObject> {
        Self::new(py, obj, meta, classname)
    }

    def __call__(&self, *args, **kwargs) -> PyResult<PyObject> {
        // Attention: make sure DATA.lock() does not overlap with Python
        // operations. "Simple" Python code like `getattr(a, b)` can
        // potentially call DATA.lock() and cause deadlock.

        // Prepare extra (dynamic) metadata.
        // This calls into Python and cannot take DATA.lock().
        let mut extra_meta: Option<Vec<(String, String)>> = None;
        if let Some(meta) = self.meta(py) {
            let meta = if meta.is_callable(py) {
                // meta: (*args, **kwargs) -> [(str, str)]
                meta.call(py, args, kwargs)?
            } else {
                // meta: [(str, str)]
                meta.clone_ref(py)
            };
            if meta == py.None() {
                // Special case: bypass logging.
                return self.inner(py).call(py, args, kwargs);
            }
            let meta = meta.extract::<Vec<(String, String)>>(py)?;
            extra_meta = Some(meta);
        }

        // Enter Span.
        let espan_id = {
            let last_id = self.last_espan_id(py).get();
            let name = self.name(py);
            let module = self.module(py);
            let line = self.lineno(py);
            let basic_meta: [(&str, &str); 3] = [("name", &name), ("module_path", &module), ("line", &line)];

            // Okay to lock - pure Rust code.
            let mut data = DATA.lock();
            let espan_id = match extra_meta {
                None => {
                    // Static metadata. Avoid dynamic allocations. Try reuse Espans.
                    data.add_espan(&basic_meta[..], Some(last_id))
                },
                Some(extra) => {
                    // Dynamic metadata.
                    let meta: Vec<(&str, &str)> = basic_meta
                        .iter()
                        .cloned()
                        .chain(extra.iter().map(|(k, v)| (k.as_ref(), v.as_ref())))
                        .collect();
                    data.add_espan(&meta, Some(last_id))
                }
            };
            data.add_action(espan_id, Action::EnterSpan);
            espan_id
        };
        self.last_espan_id(py).set(espan_id);

        // This calls into Python and cannot take DATA.lock().
        let result = self.inner(py).call(py, args, kwargs);

        // Exit Span.
        {
            // Okay to lock - pure Rust code.
            let mut data = DATA.lock();
            data.add_action(espan_id, Action::ExitSpan);
        }
        result
    }

    def spanid(&self) -> PyResult<u64> {
        Ok(self.last_espan_id(py).get().0)
    }
});

impl wrapfunc {
    fn new(
        py: Python,
        obj: PyObject,
        meta: Option<PyObject>,
        class_name: Option<String>,
    ) -> PyResult<PyObject> {
        if let Ok(wrapped) = obj.extract::<wrapfunc>(py) {
            // No need to wrap again.
            return Ok(wrapped.into_object());
        }

        // Static metadata for a function - name, module and line number.
        // To reduce cost of __call__, cache them in Rust native form.
        let code = getattr(py, &obj, "__code__");
        let mut name = tostr(py, getattr(py, &obj, "__name__"));
        let module = tostr(py, getattr(py, &obj, "__module__"));
        let lineno = tostr(py, getattr(py, &code, "co_firstlineno"));

        // If the callsite provides a class name, use it.
        if let Some(class_name) = class_name {
            name = format!("{}.{}", class_name, name);
        }

        // Function wrapping is used a lot in hg extensions (via mercurial.
        // extensions.wrapfunction). Add the module name to make it easier to
        // check what the function really is.
        // For example, `dispatch.*runcommand` might be wrapped by the undo,
        // journal, copytrace, clienttelemetry, sparse extensions.  By showing
        // `journal.runcommand`, `copytrace._runcommand` instead of
        // `runcommand`, it's easier to tell what's going on.
        if let Some(module_last_name) = module.rsplit(".").nth(0) {
            // Only keep the last part of module name. There is limited space
            // in a span, and common prefix like `edenscm.mercurial` is not
            // very interesting.
            if module_last_name != "<missing>" {
                name = format!("{}.{}", module_last_name, name);
            }
        }

        // `meta` is `[(str, str)]` or `(*args, **kwargs) -> [(str, str)]`
        // to provide dynamic metadata. It's sometimes inconvenient to
        // pass "meta" through `__new__`. So we also check the `meta`
        // attribute set by the `meta` decorator. This allows the
        // following syntax:
        //
        //    @tracing.wrapfunc
        //    @tracing.meta(...)
        //    def f(...):
        //       ...
        let meta = meta.or_else(|| obj.getattr(py, "meta").ok());

        // Pre-calculate whether this function is a generator.
        // See `inspect.isgeneratorfunction` from Python stdlib.
        let mut is_generator = false;
        if let Ok(flags) = getattr(py, &code, "co_flags").extract::<u64>(py) {
            const CO_GENERATOR: u64 = 32;
            if flags & CO_GENERATOR != 0 {
                is_generator = true;
            }
        }

        let wrapped = Self::create_instance(
            py,
            obj,
            name,
            module,
            lineno,
            meta,
            is_generator,
            Cell::new(EspanId(0)),
        )?;
        Ok(wrapped.into_object())
    }
}

