/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use serde::Deserialize;
use serde::Serialize;
use std::cmp::Ord;
use std::cmp::Ordering;
use std::cmp::PartialOrd;
use std::hash::Hash;
use std::hash::Hasher;
use std::ops::Deref;

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
