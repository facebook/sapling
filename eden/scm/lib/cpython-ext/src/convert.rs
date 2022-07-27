/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::any;
use std::any::Any;
use std::any::TypeId;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::Hash;
use std::hash::Hasher;
use std::ops::Deref;

use ::serde::Deserialize;
use ::serde::Serialize;
use cpython::FromPyObject;
use cpython::PyObject;
use cpython::PyType;
use cpython::Python;
use cpython::PythonObjectDowncastError;
use cpython::PythonObjectWithTypeObject;
use cpython::_detail::ffi;
use cpython::*;
use once_cell::sync::Lazy;
use parking_lot::RwLock;

/// Wrapper type. Converts between pure Rust bytes-like types and PyBytes.
///
/// The Rust type needs to implement `AsRef<[u8]>` and `From<Vec<u8>>`.
///
/// In bindings code:
/// - For input, use `v: BytesLike<MyType>` in definition, and `v.0` to extract
///   `MyType`.
/// - For output, use `-> BytesLike<MyType>` in definition, and `BytesLike(v)`
///   to construct the return value.
#[derive(Clone, Debug, Eq, Ord, Hash, PartialEq, PartialOrd)]
pub struct BytesLike<T>(pub T);

impl<T: AsRef<[u8]>> ToPyObject for BytesLike<T> {
    type ObjectType = PyBytes;

    fn to_py_object(&self, py: Python) -> Self::ObjectType {
        PyBytes::new(py, self.0.as_ref())
    }
}

impl<'s, T: From<Vec<u8>>> FromPyObject<'s> for BytesLike<T> {
    fn extract(py: Python, obj: &'s PyObject) -> PyResult<Self> {
        obj.extract::<PyBytes>(py)
            .map(|v| Self(v.data(py).to_vec().into()))
    }
}

impl<T> Deref for BytesLike<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Wrapper type. Converts between pure Rust serde types and PyObjct.
///
/// In bindings code:
/// - For input, use `v: Serde<MyType>` in definition, and `v.0` to extract
///   `MyType`.
/// - For output, use `-> Serde<MyType>` in definition, and `Serde(v)` to
///   construct the return value.
#[derive(Debug)]
pub struct Serde<T>(pub T);

impl<T: Serialize> ToPyObject for Serde<T> {
    type ObjectType = PyObject;

    fn to_py_object(&self, py: Python) -> Self::ObjectType {
        crate::ser::to_object(py, &self.0).unwrap()
    }
}

impl<'s, T> FromPyObject<'s> for Serde<T>
where
    T: for<'de> Deserialize<'de>,
{
    fn extract(py: Python, obj: &'s PyObject) -> PyResult<Self> {
        let inner = crate::de::from_object(py, obj.clone_ref(py))?;
        Ok(Self(inner))
    }
}

impl<T> Deref for Serde<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: PartialOrd> PartialOrd<Serde<T>> for Serde<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl<T: Ord> Ord for Serde<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl<T: Hash> Hash for Serde<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }
}

impl<T: PartialEq> PartialEq for Serde<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<T: Eq> Eq for Serde<T> {}

impl<T: Clone> Clone for Serde<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// Registered functions about converting PyObject (type decided by PyType)
/// to Box<T> (type decided by TypeId).
///
/// The key of the map is (input Python type, output Rust type).
static CONVERT_FUNC_BY_TYPE: Lazy<RwLock<HashMap<(PyTypeId, TypeId), ExtractPyObjectFunc>>> =
    Lazy::new(|| Default::default());

type ExtractPyObjectFunc =
    Box<dyn (Fn(Python, &PyObject) -> Box<dyn Any + Send + Sync>) + Send + Sync>;

/// An `usize` used to identify CPython types.
#[derive(Hash, Eq, PartialEq, Ord, PartialOrd)]
struct PyTypeId(*const ffi::PyTypeObject);

// safety: PyTypeObject is a static pointer. It can be treated as an
// integer to compare.
unsafe impl Send for PyTypeId {}
unsafe impl Sync for PyTypeId {}

impl From<&'_ PyType> for PyTypeId {
    fn from(py_type: &PyType) -> Self {
        Self(py_type.as_type_ptr() as _)
    }
}

impl PyTypeId {
    fn null() -> Self {
        Self(std::ptr::null())
    }
}

/// Register a function to convert a PyObject (Python type: P)
/// to type O using the specified function.
///
/// After registration, `ImplInto<O>` can be used in Python function
/// definitions.
///
/// If P is `PyObject`, then it matches heap types (usually, objects
/// created by instantiating a pure Python `class`). It does not
/// match non-heap types such as `PyBytes`.
pub fn register_into<P, F, O>(py: Python, convert_func: F)
where
    P: PythonObjectWithTypeObject,
    P: for<'a> FromPyObject<'a>,
    F: Fn(Python, P) -> O,
    F: Send + Sync + 'static,
    O: Send + Sync + 'static,
{
    let py_type_id = if TypeId::of::<P>() == TypeId::of::<PyObject>() {
        PyTypeId::null()
    } else {
        let py_type = P::type_object(py);
        assert!(
            !is_heap_type(py, &py_type),
            "native heap type is unsupported"
        );
        PyTypeId::from(&py_type)
    };
    let output_type_id = TypeId::of::<O>();
    let func = move |py: Python, obj: &PyObject| -> Box<dyn Any + Send + Sync> {
        let obj: P = obj.extract::<P>(py).expect("PyTypeId was checked");
        let obj: O = convert_func(py, obj);
        Box::new(obj) as Box<dyn Any + Send + Sync>
    };
    CONVERT_FUNC_BY_TYPE
        .write()
        .insert((py_type_id, output_type_id), Box::new(func));
}

/// Wrapper type. Converts `PyObject` to `T`.
///
/// How to convert `PyObject` to `T` needs to be pre-registered via `register_into`.
pub struct ImplInto<T>(pub T);

impl<'s, T> FromPyObject<'s> for ImplInto<T>
where
    T: 'static,
{
    fn extract(py: Python, obj: &'s PyObject) -> PyResult<Self> {
        let py_type = obj.get_type(py);
        let py_type_id = if is_heap_type(py, &py_type) {
            PyTypeId::null()
        } else {
            PyTypeId::from(&py_type)
        };
        let output_type_id = TypeId::of::<T>();

        let table = CONVERT_FUNC_BY_TYPE.read();
        match table.get(&(py_type_id, output_type_id)) {
            Some(convert_func) => {
                let inner: Box<T> = (convert_func)(py, obj)
                    .downcast::<T>()
                    .expect("TypeId was checked");
                Ok(ImplInto(*inner))
            }
            None => {
                let expected_type_name = any::type_name::<Self>();
                Err(PythonObjectDowncastError::new(py, expected_type_name, py_type).into())
            }
        }
    }
}

impl<T> ImplInto<T> {
    /// Convert to the inner type.
    pub fn into(self) -> T {
        self.0
    }
}

/// Test whether `typeobj` is a heap type. A heap type is usually
/// defined in pure Python using the `class` keyword and has a
/// `__dict__` slot supporting `setattr`. `py_class!` types or
/// builtin types like bytes, str and are not heap types.
fn is_heap_type(_py: Python, typeobj: &PyType) -> bool {
    let type_ptr: *mut ffi::PyTypeObject = typeobj.as_type_ptr();
    // safety: _py holds GIL. The pointer is valid.
    let result = (unsafe { *type_ptr }.tp_flags & ffi::Py_TPFLAGS_HEAPTYPE) != 0;
    result
}

#[cfg(test)]
mod tests {
    use cpython::*;

    use super::*;

    py_class!(class T1 |py| {
        data v: String;
    });

    py_class!(class T2 |py| {
        data v: i32;
    });

    #[test]
    fn test_impl_into() {
        let gil = Python::acquire_gil();
        let py = gil.python();

        // Register how to convert T1, T2 to String.
        register_into(py, |py, t: T1| t.v(py).clone());
        register_into(py, |py, t: T2| t.v(py).to_string());
        register_into(py, |_py, t: PyObject| t.to_string());

        let v1 = T1::create_instance(py, "abc".to_string()).unwrap();
        let v2 = T2::create_instance(py, 123).unwrap();
        let p1: PyObject = v1.into_object();
        let p2: PyObject = v2.into_object();

        // p3 is a heap type.
        let p3: PyObject = py.eval("type('Foo', (object,), {})()", None, None).unwrap();

        // Converting to Impl<String> works without casting to T1 or T2 first.
        let s1 = p1.extract::<ImplInto<String>>(py).unwrap();
        let s2 = p2.extract::<ImplInto<String>>(py).unwrap();
        let s3 = p3.extract::<ImplInto<String>>(py).unwrap();

        assert_eq!(s1.into(), "abc");
        assert_eq!(s2.into(), "123");
        assert_eq!(
            // strip the unstable "at 0x......" part.
            s3.into().split_whitespace().next().unwrap(),
            "<__main__.Foo"
        );

        // p4 is a builtin (native, non-heap) type that is not registered.
        let p4: PyObject = PyBytes::new(py, b"Foo").into_object();
        assert!(p4.extract::<ImplInto<String>>(py).is_err());
    }
}
