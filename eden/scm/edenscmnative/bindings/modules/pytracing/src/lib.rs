/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::ops::DerefMut;
use std::sync::Arc;

use cpython::*;
use cpython_ext::Bytes;
use cpython_ext::PyNone;
use cpython_ext::ResultPyErrExt;
use cpython_ext::Str;
use lazy_static::lazy_static;
use parking_lot::Mutex;
#[cfg(feature = "python3")]
use python3_sys as ffi;
use tracing::Level;
use tracing_collector::model::Action;
use tracing_collector::model::EspanId;
use tracing_collector::model::TreeSpans;
use tracing_collector::TracingData;
use tracing_runtime_callsite::CallsiteInfo;
use tracing_runtime_callsite::CallsiteKey;
use tracing_runtime_callsite::EventKindType;
use tracing_runtime_callsite::KindType;
use tracing_runtime_callsite::RuntimeCallsite;
use tracing_runtime_callsite::SpanKindType;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "tracing"].join(".");
    let m = PyModule::new(py, &name)?;

    m.add_class::<tracingdata>(py)?;
    m.add_class::<meta>(py)?;
    m.add_class::<wrapfunc>(py)?;
    m.add_class::<wrapiter>(py)?;
    m.add(py, "isheaptype", py_fn!(py, is_heap_type(obj: PyType)))?;

    impl_getsetattr::<wrapfunc>(py);
    impl_getsetattr::<wrapiter>(py);

    let singleton = tracingdata::create_instance(py, DATA.clone())?;
    m.add(py, "singleton", singleton)?;

    m.add(py, "LEVEL_TRACE", LEVEL_TRACE)?;
    m.add(py, "LEVEL_DEBUG", LEVEL_DEBUG)?;
    m.add(py, "LEVEL_INFO", LEVEL_INFO)?;
    m.add(py, "LEVEL_WARN", LEVEL_WARN)?;
    m.add(py, "LEVEL_ERROR", LEVEL_ERROR)?;
    m.add_class::<EventCallsite>(py)?;
    m.add_class::<SpanCallsite>(py)?;
    m.add_class::<instrument>(py)?;
    impl_getsetattr::<InstrumentFunction>(py);

    m.add(
        py,
        "updateenvfilter",
        py_fn!(py, updateenvfilter(dirs: &str)),
    )?;

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
            mincode::deserialize(bytes).map_pyerr(py)?
        } else {
            serde_json::from_slice(bytes).map_pyerr(py)?
        };
        Self::create_instance(py, Arc::new(Mutex::new(data)))
    }

    /// Serialize to bytes.
    def serialize(&self, binary: bool = true) -> PyResult<Bytes> {
        let data = self.data(py).lock();
        if binary {
            let bytes = mincode::serialize(data.deref()).map_pyerr(py)?;
            Ok(Bytes::from(bytes))
        } else {
            let json = serde_json::to_string(data.deref()).map_pyerr(py)?;
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
    def edit(&self, id: u64,  metadata: Vec<(String, String)>) -> PyResult<PyNone> {
        let mut data = self.data(py).lock();
        data.edit_espan(EspanId(id), metadata);
        Ok(PyNone)
    }

    /// Enter a span.
    def enter(&self, id: u64) -> PyResult<PyNone> {
        let mut data = self.data(py).lock();
        data.add_action(EspanId(id), Action::EnterSpan);
        Ok(PyNone)
    }

    /// Exit a span.
    def exit(&self, id: u64) -> PyResult<PyNone> {
        let mut data = self.data(py).lock();
        data.add_action(EspanId(id), Action::ExitSpan);
        Ok(PyNone)
    }

    /// Add an event.
    def event(&self, metadata: Vec<(String, String)>) -> PyResult<PyNone> {
        let metadata: Vec<(&str, &str)> = metadata.iter().map(|(k, v)| (k.as_ref(), v.as_ref())).collect();
        let mut data = self.data(py).lock();
        let id = data.add_espan(&metadata, None);
        data.add_action(id, Action::Event);
        Ok(PyNone)
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
        data.write_trace_event_json(&mut buf, Default::default()).map_pyerr(py)?;
        Ok(Bytes::from(buf))
    }

    /// Export as ASCII.
    ///
    /// `minduration` specifies the minimal duration threshold in micro seconds.
    /// The default value is 10000 (10 milliseconds).
    def ascii(&self, minduration: u64 = 10000) -> PyResult<Str> {
        let mut opts = tracing_collector::model::AsciiOptions::default();
        opts.min_duration_micros_to_hide = minduration;
        Ok(self.data(py).lock().ascii(&opts).into())
    }

    /// Get the TreeSpans for all threads.
    ///
    /// Return {(pid, tid): treespans}.
    def treespans(&self) -> PyResult<HashMap<(u64, u64), treespans>> {
        let data = self.data(py).lock();
        let spans = data.tree_spans().into_iter().map(|(k, v)|{
            (k, treespans::create_instance(py, v).expect("create_instance should succeed"))
        }).collect();
        Ok(spans)
    }

    /// Swap with the global singleton.
    def __enter__(&self) -> PyResult<PyNone> {
        self.swap_with_singleton(py);
        Ok(PyNone)
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

fn getattr_opt(py: Python, obj: &PyObject, name: &str) -> Option<PyObject> {
    obj.getattr(py, name).ok()
}

fn tostr(py: Python, obj: PyObject) -> String {
    obj.str(py)
        .map(|s| s.to_string_lossy(py).to_string())
        .unwrap_or_else(|_| "<missing>".to_string())
}

fn tostr_opt(py: Python, obj: PyObject) -> Option<String> {
    obj.str(py)
        .map(|s| Some(s.to_string_lossy(py).to_string()))
        .unwrap_or_else(|_| None)
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
    data push_callback: Option<PyObject>;
    data pop_callback: Option<PyObject>;

    def __new__(_cls,
        obj: PyObject,
        meta: Option<PyObject> = None,
        classname: Option<String> = None,
        push_callback: Option<PyObject> = None,
        pop_callback: Option<PyObject> = None
    ) -> PyResult<PyObject> {
        Self::new(py, obj, meta, classname, push_callback, pop_callback)
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

        if let Some(callback) = &self.push_callback(py) {
            callback.call(py, (espan_id.0,), None)?;
        }

        // This calls into Python and cannot take DATA.lock().
        let mut result = self.inner(py).call(py, args, kwargs);

        // Wrap generator automatically.
        if *self.is_generator(py) {
            if let Ok(ref obj) = result {
                if let Ok(obj) = wrapiter::new(py, obj.clone_ref(py)) {
                    result = Ok(obj.into_object());
                }
            }
        }

        if let Some(callback) = &self.pop_callback(py) {
            callback.call(py, NoArgs, None)?;
        }

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
        push_callback: Option<PyObject>,
        pop_callback: Option<PyObject>,
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
            // in a span, and common prefix like `edenscm` is not
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
            push_callback,
            pop_callback,
        )?;
        Ok(wrapped.into_object())
    }
}

py_class!(class wrapiter |py| {
    data inner: PyObject;
    data name: String;
    data last_espan_id: Cell<EspanId>;

    def __new__(_cls, obj: PyObject) -> PyResult<wrapiter> {
        Self::new(py, obj)
    }

    def __iter__(&self) -> PyResult<Self> {
        Ok(self.clone_ref(py))
    }

    def __next__(&self) -> PyResult<Option<PyObject>> {
        // PERF: Could be a bit faster using unsafe `ffi::PyIter_Next` directly.
        let mut iter = PyIterator::from_object(py, self.inner(py).clone_ref(py))?;

        // Enter Span.
        let espan_id = {
            let last_id = self.last_espan_id(py).get();
            let name = self.name(py);
            let meta: [(&str, &str); 2] = [("name", &name), ("cat", "generator")];
            let mut data = DATA.lock();
            let espan_id = data.add_espan(&meta[..], Some(last_id));
            data.add_action(espan_id, Action::EnterSpan);
            espan_id
        };

        // next(generator)
        let result = match iter.next() {
            None => Ok(None),
            Some(Err(err)) => Err(err),
            Some(Ok(obj)) => Ok(Some(obj)),
        };

        // Exit Span.
        {
            let mut data = DATA.lock();
            data.add_action(espan_id, Action::ExitSpan);
        }
        self.last_espan_id(py).set(espan_id);
        result
    }
});

impl wrapiter {
    fn new(py: Python, obj: PyObject) -> PyResult<Self> {
        if let Ok(wrapped) = obj.extract::<Self>(py) {
            return Ok(wrapped);
        }
        let name = format!("{}.next", tostr(py, getattr(py, &obj, "__name__")));
        Self::create_instance(py, obj, name, Cell::new(EspanId(0)))
    }
}

/// Test whether a `type` (aka. `class`) object is a heap type or not.
/// This is not exposed in Python stdlib. But can be useful to check
/// whether `setattr` is supported or not, which decides whether that
/// class can be traced or not.
///
/// Practically, a heap type is usually defined in Python land using
/// the `class` keyword. A non-heap type is usually defined using native
/// languages. A heap type usually has a `__dict__` slot so it can
/// store attributes and support `setattr`.
fn is_heap_type(_py: Python, typeobj: PyType) -> PyResult<bool> {
    let type_ptr: *mut ffi::PyTypeObject = typeobj.as_type_ptr();
    let result = (unsafe { *type_ptr }.tp_flags & ffi::Py_TPFLAGS_HEAPTYPE) != 0;
    Ok(result)
}

/// Add T.__get__ and __set__ so it can proxy attributes
/// like __doc__ to the original object.
fn impl_getsetattr<T: PythonTypeWithInner>(py: Python) {
    // rust-cpython does not provide a safe way to define __get__.
    // So we have to use some unsafe ffi.
    let type_object: PyType = T::type_object(py);
    let type_ptr: *mut ffi::PyTypeObject = type_object.as_type_ptr();

    extern "C" fn getattr<T: PythonTypeWithInner>(
        this: *mut ffi::PyObject,
        name: *mut ffi::PyObject,
    ) -> *mut ffi::PyObject {
        // This function is called by the Python interpreter.
        // So GIL is already held.
        let py = unsafe { Python::assume_gil_acquired() };

        // Convert raw ffi pointer to friendly rust-cpython objects.
        let this = unsafe { PyObject::from_borrowed_ptr(py, this) };
        let this = unsafe { T::unchecked_downcast_from(this) };
        let name = unsafe { PyObject::from_borrowed_ptr(py, name) };

        match this.inner_obj(py).getattr(py, name) {
            Err(err) => {
                err.restore(py);
                std::ptr::null_mut()
            }
            Ok(obj) => obj.steal_ptr(),
        }
    }

    extern "C" fn setattr<T: PythonTypeWithInner>(
        this: *mut ffi::PyObject,
        name: *mut ffi::PyObject,
        value: *mut ffi::PyObject,
    ) -> std::os::raw::c_int {
        let py = unsafe { Python::assume_gil_acquired() };

        // Convert raw ffi pointer to friendly rust-cpython objects.
        let this = unsafe { PyObject::from_borrowed_ptr(py, this) };
        let this = unsafe { T::unchecked_downcast_from(this) };
        let name = unsafe { PyObject::from_borrowed_ptr(py, name) };
        let value = unsafe { PyObject::from_borrowed_ptr(py, value) };

        match this.inner_obj(py).setattr(py, name, value) {
            Err(err) => {
                err.restore(py);
                -1
            }
            Ok(_) => 0,
        }
    }

    // `__get__`, useful to bind `self` to a free function.
    // See https://docs.python.org/2/howto/descriptor.html.
    extern "C" fn descr_get<T: PythonTypeWithInner>(
        this: *mut ffi::PyObject,
        obj: *mut ffi::PyObject,
        typeobj: *mut ffi::PyObject,
    ) -> *mut ffi::PyObject {
        let py = unsafe { Python::assume_gil_acquired() };

        let this = unsafe { PyObject::from_borrowed_ptr(py, this) };
        let this = unsafe { T::unchecked_downcast_from(this) };
        let inner = this.inner_obj(py);

        // Need to call inner->ob_type->tp_descr_get.
        // There does not seem to have a Python API for calling this.
        let inner_type = inner.get_type(py);
        let inner_type_ptr: *mut ffi::PyTypeObject = inner_type.as_type_ptr();
        let inner_descr_get = unsafe { *inner_type_ptr }.tp_descr_get;
        if let Some(descr_get) = inner_descr_get {
            // Delegate to the original descr_get implementation.
            // Most interestingly, `func_descr_get` in `funcobject.c`.
            let result: *mut ffi::PyObject =
                unsafe { descr_get(this.into_object().steal_ptr(), obj, typeobj) };
            result
        } else {
            this.into_object().steal_ptr()
        }
    }

    // Modify the type object to make __get__ working.
    unsafe {
        (*type_ptr).tp_getattro = Some(getattr::<T>);
        (*type_ptr).tp_setattro = Some(setattr::<T>);
        (*type_ptr).tp_descr_get = Some(descr_get::<T>);
    }
}

trait PythonTypeWithInner: PythonObjectWithTypeObject + PythonObject {
    fn inner_obj<'a>(&'a self, _py: Python<'a>) -> &'a PyObject;
}

impl PythonTypeWithInner for wrapfunc {
    fn inner_obj<'a>(&'a self, py: Python<'a>) -> &'a PyObject {
        self.inner(py)
    }
}

impl PythonTypeWithInner for wrapiter {
    fn inner_obj<'a>(&'a self, py: Python<'a>) -> &'a PyObject {
        self.inner(py)
    }
}

py_class!(pub class treespans |py| {
    data spans: TreeSpans<String>;

    /// Convert into plain Python objects:
    ///
    /// [{"start": micros, "duration": micros | null, "children": [index], **meta}]
    def flatten(&self) -> PyResult<PyObject> {
        cpython_ext::ser::to_object(py, self.spans(py))
    }

    /// Get a "flat" list of spans by name.
    def byname(&self, name: String) -> PyResult<PyObject> {
        let spans = self.spans(py).walk().filter(move |walker, span| {
            if span.meta.get("name") == Some(&name) {
                walker.step_out();
                true
            } else {
                false
            }
        }).collect::<Vec<_>>();
        cpython_ext::ser::to_object(py, &spans)
    }
});

py_class!(pub class TracingSpan |py| {
    // The fields of a struct are dropped in declaration order.
    // "entered" is borrowed from "span". Declare it first.
    data entered: RefCell<Option<UnsafePyLeaked<tracing::span::Entered<'static>>>>;
    @shared data span: tracing::Span;

    def __enter__(&self) -> PyResult<Self> {
        let mut entered = self.entered(py).borrow_mut();
        if entered.is_some() {
            Err(PyErr::new::<exc::ValueError, _>(py, "tracing span was already entered"))
        } else {
            // safety: "entered" lifetime won't exceed its parent (Span).
            *entered = Some(unsafe {
                self.span(py).leak_immutable().map(py, |s| { s.enter() } )
            });
            Ok(self.clone_ref(py))
        }
    }

    def __exit__(&self, _ty: Option<PyType>, _value: PyObject, _traceback: PyObject) -> PyResult<bool> {
        let mut entered = self.entered(py).borrow_mut();
        *entered = None;
        Ok(false) // Do not suppress exception
    }

    /// Record a value. The field name must be predefined with the callsite.
    def record(&self, name: &str, value: FieldValue) -> PyResult<PyNone> {
        if let Some(value) = value.to_opt_tracing_value() {
            self.span(py).borrow().record(name, &value.as_ref());
        }
        Ok(PyNone)
    }

    /// Returns true if this span was disabled by the subscriber.
    def is_disabled(&self) -> PyResult<bool> {
        Ok(self.span(py).borrow().is_disabled())
    }

    /// Returns this span's Id, if it is enabled.
    def id(&self) -> PyResult<Option<u64>> {
        Ok(self.span(py).borrow().id().map(|i| i.into_u64()))
    }
});

py_class!(pub class SpanCallsite |py| {
    data inner: &'static RuntimeCallsite<SpanKindType>;

    // Create a `Callsite` for span in Rust tracing eco-system.
    def __new__(
        _cls,
        obj: PyObject, /* func, or str */
        target: Option<String> = None,
        name: Option<String> = None,
        level: usize = LEVEL_INFO,
        file: Option<String> = None,
        line: Option<usize> = None,
        module: Option<String> = None,
        fieldnames: Option<Vec<String>> = None,
    ) -> PyResult<Self> {
        let callsite = new_callsite(py, obj, target, name, level, file, line, module, fieldnames)?;
        Self::create_instance(py, callsite)
    }

    /// Create a span with the given field values.
    def span(&self, values: Option<Vec<FieldValue>> = None) -> PyResult<TracingSpan> {
        // Convert values to Option<Box<dyn tracing::Value>>.
        let values: Vec<Option<Box<dyn tracing::Value>>> = match values.as_ref() {
            None => Vec::new(),
            Some(list) => list.iter().map(|v| v.to_opt_tracing_value()).collect(),
        };
        let span = self.inner(py).create_span(&values);
        TracingSpan::create_instance(py, Default::default(), span)
    }

    /// Check if this callsite is enabled for logging.
    def isenabled(&self) -> PyResult<bool> {
        Ok(self.inner(py).is_enabled())
    }
});

py_class!(pub class EventCallsite |py| {
    data inner: &'static RuntimeCallsite<EventKindType>;

    // Create a `Callsite` for event in Rust tracing eco-system.
    def __new__(
        _cls,
        obj: PyObject, /* id(obj) for identity */
        target: Option<String> = None,
        name: Option<String> = None,
        level: usize = LEVEL_INFO,
        file: Option<String> = None,
        line: Option<usize> = None,
        module: Option<String> = None,
        fieldnames: Option<Vec<String>> = None,
    ) -> PyResult<Self> {
        let callsite = new_callsite(py, obj, target, name, level, file, line, module, fieldnames)?;
        Self::create_instance(py, callsite)
    }

    /// Create a event with the given field values.
    def event(&self, values: Option<Vec<FieldValue>> = None) -> PyResult<PyNone> {
        // Convert values to Option<Box<dyn tracing::Value>>.
        let values: Vec<Option<Box<dyn tracing::Value>>> = match values.as_ref() {
            None => Vec::new(),
            Some(list) => list.iter().map(|v| v.to_opt_tracing_value()).collect(),
        };
        self.inner(py).create_event(&values);
        Ok(PyNone)
    }

    /// Check if this callsite is enabled for logging.
    def isenabled(&self) -> PyResult<bool> {
        Ok(self.inner(py).is_enabled())
    }
});

/// Create a runtime callsite.
///
/// `obj` should be a Python function or frame so we can figure out the right
/// "callsite" identity and fill up information like module, file, line, etc.
fn new_callsite<K: KindType>(
    py: Python,
    obj: PyObject, /* func, or str */
    target: Option<String>,
    name: Option<String>,
    level: usize,
    file: Option<String>,
    line: Option<usize>,
    module: Option<String>,
    fieldnames: Option<Vec<String>>,
) -> PyResult<&'static RuntimeCallsite<K>> {
    enum ObjType {
        Frame,
        Func,
    }

    // Extract the "code" object and "line". Support frame and function object.
    let (code, line, obj_type) = if let Some(code) = getattr_opt(py, &obj, "f_code") {
        // obj is a frame object (f_code, f_lineno, f_globals, etc.)
        let line = line.or_else(|| getattr(py, &obj, "f_lineno").extract::<usize>(py).ok());
        (code, line, ObjType::Frame)
    } else if let Some(code) = getattr_opt(py, &obj, "__code__") {
        // obj is a function object (__code__, __name__, __module__, etc.)
        let line = line.or_else(|| {
            getattr(py, &obj, "co_firstlineno")
                .extract::<usize>(py)
                .ok()
        });
        (code, line, ObjType::Func)
    } else {
        return Err(PyErr::new::<exc::TypeError, _>(
            py,
            "callsite: expected frame or function object",
        ));
    };
    let id: CallsiteKey = (code.as_ptr() as usize, line.unwrap_or_default());

    let callsite = tracing_runtime_callsite::create_callsite::<K, _>(id, || {
        // Populate other fields: name, module, file.
        let (mut name, module) = match obj_type {
            ObjType::Frame => {
                let name = name.or_else(|| tostr_opt(py, getattr(py, &code, "co_name")));
                let module = module.or_else(|| {
                    getattr_opt(py, &obj, "f_globals").and_then(|g| {
                        g.get_item(py, "__name__")
                            .ok()
                            .and_then(|n| n.extract::<String>(py).ok())
                    })
                });
                (name, module)
            }
            ObjType::Func => {
                let name = name.or_else(|| tostr_opt(py, getattr(py, &obj, "__name__")));
                let module = module.or_else(|| tostr_opt(py, getattr(py, &obj, "__module__")));
                (name, module)
            }
        };

        // Discard the "<lambda>" name. It's pointless.
        if name.as_deref() == Some("<lambda>") {
            name = None;
        };

        // code object provides file name.
        let file = file.or_else(|| tostr_opt(py, getattr(py, &code, "co_filename")));

        CallsiteInfo {
            name: name.unwrap_or_default(),
            // Rewrite Python module "foo.bar" to Rust form "foo::bar". The `.` is not supported
            // by env filter syntax.
            target: target
                .unwrap_or_else(|| module.as_deref().unwrap_or_default().replace('.', "::")),
            level: usize_to_level(level),
            file,
            line: line.map(|l| l as u32),
            module_path: module,
            field_names: fieldnames.unwrap_or_default(),
        }
    });
    Ok(callsite)
}

// Instrument decorator. Construct a callable that accepts a function
// and returns InstrumentFunction
py_class!(class instrument |py| {
    data name: Option<String>;
    data target: Option<String>;
    data level: usize;
    data skip: Option<Vec<String>>;
    data meta: Option<Vec<(String, FieldValue)>>;

    def __new__(_cls, *args, **kwargs) -> PyResult<PyObject> {
        let decorator = {
            let get = |name| -> Option<PyObject> { kwargs?.get_item(py, name) };
            let get_str = |name| -> Option<String> { get(name)?.extract(py).ok() };
            let name: Option<String> = get_str("name");
            let target: Option<String> = get_str("target");
            let level: usize = get("level").and_then(|l| l.extract(py).ok()).unwrap_or(LEVEL_INFO);
            let skip: Option<Vec<String>> = get("skip").and_then(|l| l.extract(py).ok());
            let meta: Option<Vec<(String, FieldValue)>> = kwargs.map(|kwargs| {
                // The rest of the kwargs.
                kwargs.items(py).into_iter().filter_map(|(name, value)| -> Option<_> {
                    let name = name.extract::<String>(py).ok()?;
                    if name == "name" || name == "target" || name == "level" || name == "skip" {
                        None
                    } else {
                        let value = value.extract::<FieldValue>(py).ok()?;
                        Some((name, value))
                    }
                }).collect()
            });
            Self::create_instance(py, name, target, level, skip, meta)?
        };

        match args.len(py) {
            0 => Ok(decorator.into_object()),
            1 => {
                // Single function: decoratoring function
                let func = args.get_item(py, 0);
                let decorated = decorator.__call__(py, func)?;
                Ok(decorated.into_object())
            }
            _ => {
                Err(PyErr::new::<exc::TypeError, _>(py, "instrument expect 0 or 1 args"))
            }
        }
    }

    // Decorate a function. Analyze its arguments.
    def __call__(&self, func: PyObject) -> PyResult<InstrumentFunction> {
        // See inspect.getargs for how to extract arguments.
        let code = func.getattr(py, "__code__")?;

        // Predefined fields (name, value). Includes instrumented args and meta.
        let fields: Vec<(String, InstrumentFieldValue)> = {
            // "static" normal parameters (no *args, **kwds) that are also not skipped.
            // "names" include args and kwargs.
            let names: Vec<String> = code.getattr(py, "co_varnames")?.extract(py)?;
            // "count" only includes normal parameters (no args, or kwargs).
            let count: usize = code.getattr(py, "co_argcount")?.extract(py)?;
            let is_skipped: Box<dyn Fn(&String) -> bool> = match self.skip(py) {
                None => Box::new(|_n| false),
                Some(skip) => Box::new(move |n| skip.contains(n)),
            };
            let mut fields: Vec<(String, InstrumentFieldValue)> =
                names.into_iter().enumerate().take(count).filter_map(|(i, n)| {
                    if is_skipped(&n) {
                        None
                    } else {
                        Some((n, InstrumentFieldValue::FunctionArg(i)))
                    }
                }).collect();
            // Also include names from the metadata.
            if let Some(meta) = self.meta(py) {
                for (key, value) in meta.clone() {
                    fields.push((key, InstrumentFieldValue::Value(value)));
                }
            }
            fields
        };

        let field_names: Vec<String> = fields.iter().map(|t| t.0.clone()).collect();
        let callsite = new_callsite::<SpanKindType>(
            py,
            func.clone_ref(py),
            self.target(py).clone(),
            self.name(py).clone(),
            *self.level(py),
            None /* file */,
            None /* line */,
            None /* module */,
            Some(field_names),
        )?;

        let field_values: Vec<InstrumentFieldValue> = fields.into_iter().map(|t| t.1).collect();
        InstrumentFunction::create_instance(py, func, callsite, field_values)
    }
});

// Instrumented (Python) function.
//
// This is similar to wrapfunc with the differences:
// - it writes to tracing eco-system instead of TracingData (more overhead since
//   spans are recreated every time (?), but level filtering works)
// - it supports logging arguments.
// - it does not wrap iterable or returned iterables.
py_class!(class InstrumentFunction |py| {
    data inner: PyObject;
    data callsite: &'static RuntimeCallsite<SpanKindType>;
    data field_values: Vec<InstrumentFieldValue>;

    def __call__(&self, *args, **kwargs) -> PyResult<PyObject> {
        // Prepare a span. To do so we need field values.
        let owned_values: Vec<FieldValue> = self.field_values(py).iter().map(|v| {
             match v {
                InstrumentFieldValue::FunctionArg(i) => {
                    let i: usize = *i;
                    if args.len(py) <= i {
                        FieldValue::None
                    } else {
                        let arg = args.get_item(py, i);
                        arg.extract::<FieldValue>(py).unwrap_or(FieldValue::None)
                    }
                },
                InstrumentFieldValue::Value(v) => v.clone(),
            }
        }).collect();
        let values: Vec<Option<Box<dyn tracing::Value>>> =
            owned_values.iter().map(|v| v.to_opt_tracing_value()).collect();

        let span = self.callsite(py).create_span(&values);
        let _entered = span.enter();

        self.inner(py).call(py, args, kwargs)
    }
});

impl PythonTypeWithInner for InstrumentFunction {
    fn inner_obj<'a>(&'a self, py: Python<'a>) -> &'a PyObject {
        self.inner(py)
    }
}

/// FieldValue for instrumented function.
/// Could be a function parameter, or a predefined FieldValue.
#[derive(Debug)]
enum InstrumentFieldValue {
    FunctionArg(usize),
    Value(FieldValue),
}

/// Represent the value type of a field without references.
/// Conceptually the owned value of `tracing::Value`.
#[derive(Clone, Debug)]
pub enum FieldValue {
    Str(String),
    Int(i64),
    None,
}

impl FieldValue {
    fn to_opt_tracing_value<'a>(&'a self) -> Option<Box<dyn tracing::Value + 'a>> {
        match self {
            FieldValue::Str(s) => Some(Box::new(s.as_str())),
            FieldValue::Int(i) => Some(Box::new(i)),
            FieldValue::None => None,
        }
    }
}

impl<'a> FromPyObject<'a> for FieldValue {
    fn extract(py: Python, obj: &PyObject) -> PyResult<FieldValue> {
        if let Ok(i) = obj.extract::<i64>(py) {
            Ok(FieldValue::Int(i))
        } else if obj == &py.None() {
            Ok(FieldValue::None)
        } else {
            let s = obj.extract::<String>(py)?;
            Ok(FieldValue::Str(s))
        }
    }
}

fn usize_to_level(level: usize) -> Level {
    match level {
        LEVEL_TRACE => Level::TRACE,
        LEVEL_DEBUG => Level::DEBUG,
        LEVEL_INFO => Level::INFO,
        LEVEL_WARN => Level::WARN,
        LEVEL_ERROR => Level::ERROR,
        _ => Level::ERROR,
    }
}

const LEVEL_TRACE: usize = 0;
const LEVEL_DEBUG: usize = 1;
const LEVEL_INFO: usize = 2;
const LEVEL_WARN: usize = 3;
const LEVEL_ERROR: usize = 4;

fn updateenvfilter(py: Python, dirs: &str) -> PyResult<PyNone> {
    tracing_reload::update_env_filter_directives(dirs).map_pyerr(py)?;
    Ok(PyNone)
}
