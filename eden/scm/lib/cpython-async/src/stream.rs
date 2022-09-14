/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;

use cpython::*;
use cpython_ext::AnyhowResultExt;
use cpython_ext::ResultPyErrExt;
use cpython_ext::Str;
use futures::stream::BoxStream;
use futures::stream::TryStreamExt;
use futures::Stream;

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
/// // Receive a stream (iterable) from Python:
/// fn map_reverse_stream(py: Python, tstream: S) -> PyResult<S> {
///     // Use `.stream()` to extract the pure Rust stream object
///     // that implements `Stream`.
///     let stream = tstream.stream();
///     let stream = stream.map_ok(|mut x| { x.reverse(); x });
///     Ok(stream.into())
/// }
/// ```
///
/// In Python, the stream can be used as an iterator, functions accepting
/// streams also accepts iterators:
///
/// ```python,ignore
/// stream = rustmod.produce_stream()
/// stream = rustmod.map_reverse_stream(stream)
/// for value in stream:
///     print(value)
/// ```
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

mod pytypes {
    use super::*;

    // Convert from a Python iterable to TStream.
    impl<'s, T> FromPyObject<'s> for TStream<anyhow::Result<T>>
    where
        T: for<'b> FromPyObject<'b> + Send + 'static,
    {
        fn extract(py: Python, obj: &'s PyObject) -> PyResult<Self> {
            let pyiter = obj.iter(py)?.into_object();
            let iter = itertools::unfold(pyiter, |pyiter| {
                let item = (|pyiter: &PyObject| -> PyResult<Option<T>> {
                    let gil = Python::acquire_gil();
                    let py = gil.python();
                    let mut iter = pyiter.iter(py)?;
                    if let Some(v) = iter.next() {
                        Ok(Some(v?.extract::<T>(py)?))
                    } else {
                        Ok(None)
                    }
                })(&pyiter);
                item.into_anyhow_result().transpose()
            });
            // async_runtime::iter_to_stream supports blocking `next` calls.
            // futures::stream::iter doesn't. If futures::stream::iter is used,
            // then test_nested_stream_to_and_from_python() will hang.
            let stream = async_runtime::iter_to_stream(iter);
            return Ok(stream.into());
        }
    }

    // Convert TStream to a Python object.
    impl<T> ToPyObject for TStream<anyhow::Result<T>>
    where
        T: ToPython,
    {
        type ObjectType = PyObject;

        fn to_py_object(&self, _py: Python) -> Self::ObjectType {
            panic!("bug: TStream::to_py_object should not be used");
        }

        fn into_py_object(self, py: Python) -> Self::ObjectType {
            // Erase the type. Do not convert to PyObject directly to avoid GIL cost.
            // ('py' cannot be used in the stream 'map' closure).
            let inner = self.0.map_ok(|t| Box::new(t) as Box<dyn ToPython>);
            let typename = std::any::type_name::<T>().to_string();
            let result =
                pytypes::stream::create_instance(py, RefCell::new(Some(Box::pin(inner))), typename);
            result.unwrap().into_object()
        }
    }

    /// Like `ToPyObject` but without `type Target`.
    pub trait ToPython: Send + 'static {
        fn to_py(&self, py: Python) -> PyObject;
    }

    impl<T: ToPyObject + Send + 'static> ToPython for T {
        fn to_py(&self, py: Python) -> PyObject {
            self.to_py_object(py).into_object()
        }
    }

    py_class!(pub class stream |py| {
        data state: RefCell<Option<BoxStream<'static, anyhow::Result<Box<dyn ToPython>>>>>;
        data type_name: String;

        def __iter__(&self) -> PyResult<streamiter> {
            let state = self.state(py).borrow_mut().take();
            match state {
                Some(state) => {
                    let iter = async_runtime::stream_to_iter(state);
                    streamiter::create_instance(py, RefCell::new(Box::new(iter)))
                }
                None => {
                    Err(PyErr::new::<exc::ValueError, _>(py, "stream was consumed"))
                }
            }
        }

        def typename(&self) -> PyResult<Str> {
            Ok(self.type_name(py).clone().into())
        }
    });

    py_class!(pub class streamiter |py| {
        data iter: RefCell<Box<dyn Iterator<Item = anyhow::Result<Box<dyn ToPython>>> + Send>>;

        def __next__(&self) -> PyResult<Option<PyObject>> {
            // safety: py.allow_threads runs in the same thread.
            // Its 'Send' requirement is just to disallow passing 'py'.
            struct ForceSend<T>(T);
            unsafe impl<T> Send for ForceSend<T> {}
            let iter = ForceSend(self.iter(py).borrow_mut());
            // py.allow_threads is needed because iter.next might take Python GIL.
            let next = py.allow_threads(|| {
                let mut iter = iter; // capture ForceSend into closure
                iter.0.next()
            });
            match next {
                None => Ok(None),
                Some(result) => {
                    let v = result.map_pyerr(py)?;
                    let obj = v.to_py(py);
                    Ok(Some(obj))
                }
            }
        }

        def __iter__(&self) -> PyResult<Self> {
            Ok(self.clone_ref(py))
        }
    });
}

#[cfg(test)]
mod tests {
    use futures::stream::StreamExt;

    use super::*;

    #[tokio::test]
    async fn test_stream_from_python() {
        let tstream: TStream<anyhow::Result<usize>> = with_py(|py| {
            let input = vec![3, 10, 20].into_py_object(py).into_object();
            input.extract(py).unwrap()
        });
        let mut stream = tstream.stream();
        assert_eq!(stream.next().await.transpose().unwrap(), Some(3));
        assert_eq!(stream.next().await.transpose().unwrap(), Some(10));
        assert_eq!(stream.next().await.transpose().unwrap(), Some(20));
        assert_eq!(stream.next().await.transpose().unwrap(), None);
        assert_eq!(stream.next().await.transpose().unwrap(), None);
    }

    #[test]
    fn test_stream_to_python() {
        let orig_vec = vec![5, 20, 10];
        let stream = futures::stream::iter(orig_vec.clone());
        let tstream: TStream<anyhow::Result<usize>> = stream.map(Ok).into();
        let new_vec: Vec<usize> = with_py(|py| {
            let pyobj = tstream.into_py_object(py);
            let pyiter = pyobj.iter(py).unwrap();
            pyiter
                .map(|r| with_py(|py| r.unwrap().extract::<usize>(py).unwrap()))
                .collect()
        });
        assert_eq!(new_vec, orig_vec);
    }

    #[tokio::test]
    async fn test_nested_stream_to_and_from_python() {
        let pyplus1 = with_py(|py| {
            py.run(
                r#"def plus1(stream):
                       for item in stream:
                           if item == 5:
                               raise RuntimeError("dislike this number")
                           yield item + 1"#,
                None,
                None,
            )
            .unwrap();
            py.eval("plus1", None, None).unwrap()
        });

        let stream = futures::stream::iter((5..9).rev());
        let tstream: TStream<anyhow::Result<usize>> = stream.map(Ok).into();

        let plus1 = |tstream: TStream<anyhow::Result<usize>>| -> TStream<anyhow::Result<usize>> {
            with_py(|py| {
                // TStream -> PyObject
                let arg = tstream.into_py_object(py);
                let obj: PyObject = pyplus1.call(py, (arg,), None).unwrap();
                // PyObject -> TStream
                obj.extract(py).unwrap()
            })
        };

        let tstream = plus1(tstream);
        let tstream = plus1(tstream);
        let tstream = plus1(tstream);

        let mut stream = tstream.stream();
        assert_eq!(stream.next().await.map(|v| v.unwrap()), Some(11));
        assert_eq!(stream.next().await.map(|v| v.unwrap()), Some(10));
        assert_eq!(stream.next().await.map(|v| v.unwrap()), Some(9));

        let err = stream.next().await.unwrap().unwrap_err();
        with_py(|_py| {
            assert!(format!("{}", err).contains("dislike this number"));
        });

        // The traceback includes the nested (3) functions.
        let traceback: String = with_py(|py| {
            let pyerr = Err::<u8, _>(err).map_pyerr(py).unwrap_err();
            let traceback: PyObject = pyerr.ptraceback.unwrap();
            let formatted = py
                .import("traceback")
                .unwrap()
                .call(py, "format_tb", (traceback,), None)
                .unwrap();
            formatted.extract::<Vec<String>>(py).unwrap().concat()
        });
        assert_eq!(
            traceback,
            r#"  File "<string>", line 2, in plus1
  File "<string>", line 2, in plus1
  File "<string>", line 4, in plus1
"#
        );

        assert_eq!(stream.next().await.map(|v| v.unwrap()), None);
        assert_eq!(stream.next().await.map(|v| v.unwrap()), None);
    }

    fn with_py<R>(f: impl FnOnce(Python) -> R) -> R {
        let gil = Python::acquire_gil();
        let py = gil.python();
        f(py)
    }
}
