/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython_ext::cpython::*;
use futures::stream::BoxStream;
use futures::Stream;

#[allow(clippy::needless_doctest_main)]
/// `TStream` is a thin wrapper of `Stream` from async Rust to Python.
///
/// `TStream` can be used as both input or output parameters in cpython binding
/// functions:
///
/// ```
/// # use cpython_async::{*, cpython::*, futures::*};
/// type S = TStream<anyhow::Result<Vec<u8>>>;
///
/// // Pass a stream to Python:
/// fn produce_stream(py: Python) -> PyResult<S> {
///     Ok(futures::stream::once(async { Ok(vec![1]) }).into())
/// }
///
/// // Receive a stream from Python:
/// fn map_reverse_stream(py: Python, tstream: S) -> PyResult<S> {
///     // Use `.stream()` to extract the pure Rust stream object
///     // that implements `Stream`.
///     let stream = tstream.stream();
///     let stream = stream.map_ok(|mut x| { x.reverse(); x });
///     Ok(stream.into())
/// }
/// ```
///
/// In Python, the stream can be passed around, or used as an iterator:
///
/// ```python,ignore
/// stream = rustmod.produce_stream()
/// stream = rustmod.map_reverse_stream(stream)
/// for value in stream:
///     print(value)
/// ```
///
/// To implement `TStream` for a customized type `T`, first implement
/// `ToPyObject` for `T`, then use the `py_stream_class` macro:
///
/// ```
/// # use cpython_async::{*, cpython::*, futures::*};
/// pub struct MyType(bool);
/// impl ToPyObject for MyType {
///     type ObjectType = PyBool;
///     fn to_py_object(&self, py: Python) -> Self::ObjectType { self.0.to_py_object(py) }
/// }
///
/// py_stream_class!(mod mypyclass { super::MyType });
/// # fn main() { } // needed since 'mod' cannot be inside a function.
/// ```
///
/// The Python types are defined in the `mypyclass` module.
/// `TStream<T>` will be converted to or from those types automatically
/// when crossing the Python / Rust boundary. There is no need to use
/// the types in `mypyclass` directly in Rust, instead, just use
/// `TStream<T>`.
pub struct TStream<T>(BoxStream<'static, T>);

impl<I> TStream<I> {
    /// Converts to `BoxStream` which implements the `Stream` trait.
    pub fn stream(self) -> BoxStream<'static, I> {
        self.0
    }
}

// This is convenient but prevents TStream from implementing Stream.
impl<S, I> From<S> for TStream<I>
where
    S: Stream<Item = I> + Send + 'static,
{
    fn from(s: S) -> Self {
        TStream(Box::pin(s))
    }
}

/// Defines how to convert from a Python object to
/// `TStream<anyhow::Result<Self>>`.
///
/// Implement this trait to make `TStream<anyhow::Result<Self>>` implement
/// `FromPyObject`.
///
/// This trait exists as a workaround to Rust's orphan rule - foreign crates
/// cannot implement `FromPyObject` for `TStream<ForeignType>` (E0117).
pub trait PyStreamFromPy: Send {
    /// Converts a Python object to TStream.
    fn pyobj_to_tstream(py: Python, obj: &PyObject) -> PyResult<TStream<anyhow::Result<Self>>>
    where
        Self: Sized;
}

/// Defines how to convert `TStream<anyhow::Result<Self>>` to a Python object.
///
/// Implement this trait to make `TStream<anyhow::Result<Self>>` implement
/// `ToPyObject`.
///
/// This trait exists as a workaround to Rust's orphan rule - foreign crates
/// cannot implement `ToPyObject` for `TStream<ForeignType>` (E0117).
pub trait PyStreamToPy: Send {
    /// Converts TStream to a Python object.
    fn tstream_to_pyobj(py: Python, tstream: TStream<anyhow::Result<Self>>) -> PyObject
    where
        Self: Sized;
}

impl<'s, T> FromPyObject<'s> for TStream<Result<T, anyhow::Error>>
where
    T: PyStreamFromPy,
{
    fn extract(py: Python, obj: &'s PyObject) -> PyResult<Self> {
        PyStreamFromPy::pyobj_to_tstream(py, obj)
    }
}

impl<T> ToPyObject for TStream<anyhow::Result<T>>
where
    T: PyStreamToPy,
{
    type ObjectType = PyObject;

    fn to_py_object(&self, _py: Python) -> Self::ObjectType {
        panic!("bug: TStream::to_py_object should not be used");
    }

    fn into_py_object(self, py: Python) -> Self::ObjectType {
        PyStreamToPy::tstream_to_pyobj(py, self)
    }
}

/// Macro to define Python classes for a concrete stream type. Macro is used
/// because Python types do not support static type parameters and dynamic
/// typed streams are harder to work with.
///
/// For example, `py_stream_class!(mod foomod { Foo })` defines two types in
/// the `foomod` module:
/// - `stream`: Python type that `TStream<anyhow::Result<Foo>>` converts to.
/// - `streamiter`: Python type that `iter(stream)` returns.
///
/// The type (ex. `Foo`) needs to implement `ToPyObject` so the Python
/// iterator can produce actual Python objects.
///
/// The defined types can be converted to `TStream` losslessly. In bindings
/// code, just use `TStream` in function signatures. They are pure Rust and
/// do not need `py`.
#[macro_export]
macro_rules! py_stream_class {
    (mod $m:ident { $t:ty }) => {
        mod $m {
            use $crate::PyStreamFromPy;
            use $crate::PyStreamToPy;
            use $crate::TStream;
            use cpython_ext::cpython::*;
            use std::cell::RefCell;
            use cpython_ext::ResultPyErrExt;
            use cpython_ext::Str;
            use cpython_ext::cpython::py_class;

            type T = $t;
            type E = $crate::anyhow::Error;

            impl PyStreamFromPy for T {
                fn pyobj_to_tstream(py: Python, obj: &PyObject) -> PyResult<TStream<anyhow::Result<Self>>> {
                    let py_stream = obj.extract::<stream>(py)?;
                    let mut state = None;
                    std::mem::swap(&mut state, &mut py_stream.state(py).borrow_mut());
                    match state {
                        Some(stream) => Ok(stream),
                        None => Err(PyErr::new::<exc::ValueError, _>(py, "stream was consumed")),
                    }
                }
            }

            impl PyStreamToPy for T {
                fn tstream_to_pyobj(py: Python, tstream: TStream<anyhow::Result<Self>>) -> PyObject {
                    stream::create_instance(py, RefCell::new(Some(tstream)))
                        .unwrap()
                        .into_object()
                }
            }

            py_class!(pub class stream |py| {
                data state: RefCell<Option<TStream<Result<T, E>>>>;

                def __iter__(&self) -> PyResult<streamiter> {
                    let tstream: TStream<Result<T, E>> = self.clone_ref(py).into_object().extract(py)?;
                    let iter = $crate::async_runtime::stream_to_iter(tstream.stream());
                    streamiter::create_instance(py, RefCell::new(Box::new(iter)))
                }

                def typename(&self) -> PyResult<Str> {
                    Ok(std::any::type_name::<T>().to_string().into())
                }
            });

            py_class!(pub class streamiter |py| {
                data iter: RefCell<Box<dyn Iterator<Item = Result<T, E>> + Send>>;

                def __next__(&self) -> PyResult<Option<T>> {
                    let mut iter = self.iter(py).borrow_mut();
                    iter.next().transpose().map_pyerr(py)
                }

                def __iter__(&self) -> PyResult<Self> {
                    Ok(self.clone_ref(py))
                }
            });
        }
    }
}

// Define some common types.
py_stream_class!(mod bytes { Vec<u8> });
py_stream_class!(mod pybytes { PyBytes });
py_stream_class!(mod string { String });
