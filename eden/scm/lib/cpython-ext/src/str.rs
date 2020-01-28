/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::Bytes;
use cpython::*;

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Default, Hash, Ord)]
pub struct Str(crate::Bytes);

impl ToPyObject for Str {
    #[cfg(feature = "python3")]
    type ObjectType = PyUnicode;
    #[cfg(feature = "python2")]
    type ObjectType = PyBytes;

    #[inline]
    fn to_py_object(&self, py: Python) -> Self::ObjectType {
        #[cfg(feature = "python3")]
        return PyUnicode::new(py, &std::str::from_utf8(self.0.as_ref()).unwrap());

        #[cfg(feature = "python2")]
        self.0.to_py_object(py)
    }
}

impl From<Bytes> for Str {
    fn from(b: Bytes) -> Str {
        Str(b)
    }
}
