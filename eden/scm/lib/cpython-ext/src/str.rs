/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;

use crate::Bytes;

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Default, Hash, Ord)]
pub struct Str(crate::Bytes);

impl ToPyObject for Str {
    #[cfg(feature = "python3")]
    type ObjectType = PyUnicode;

    #[inline]
    fn to_py_object(&self, py: Python) -> Self::ObjectType {
        #[cfg(feature = "python3")]
        PyUnicode::new(py, std::str::from_utf8(self.0.as_ref()).unwrap())
    }
}

impl From<Bytes> for Str {
    fn from(b: Bytes) -> Str {
        Str(b)
    }
}

impl From<String> for Str {
    fn from(s: String) -> Str {
        Str(Bytes::from(s))
    }
}

impl From<Vec<u8>> for Str {
    fn from(s: Vec<u8>) -> Str {
        Str(Bytes::from(s))
    }
}
