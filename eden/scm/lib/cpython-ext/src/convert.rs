/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
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
