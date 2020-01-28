/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    convert::{TryFrom, TryInto},
    path::{Path, PathBuf},
};

#[cfg(feature = "python2")]
use anyhow::format_err;
use anyhow::Result;
use cpython::*;

#[cfg(feature = "python2")]
use encoding::{local_bytes_to_path, path_to_local_bytes};

#[cfg(feature = "python2")]
use crate::ResultPyErrExt;

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Default, Hash, Ord)]
pub struct PyPath(String);

impl PyPath {
    pub fn to_path_buf(&self) -> PathBuf {
        Path::new(&self.0).to_path_buf()
    }
}

impl ToPyObject for PyPath {
    #[cfg(feature = "python3")]
    type ObjectType = PyUnicode;
    #[cfg(feature = "python2")]
    type ObjectType = PyBytes;

    #[inline]
    fn to_py_object(&self, py: Python) -> Self::ObjectType {
        #[cfg(feature = "python3")]
        return self.0.to_py_object(py);

        #[cfg(feature = "python2")]
        PyBytes::new(py, &path_to_local_bytes(self.0.as_ref()).unwrap())
    }
}

impl<'source> FromPyObject<'source> for PyPath {
    fn extract(py: Python, obj: &'source PyObject) -> PyResult<Self> {
        #[cfg(feature = "python3")]
        {
            let s = obj.cast_as::<PyUnicode>(py)?.data(py);
            Ok(Self(s.to_string(py)?.into()))
        }

        #[cfg(feature = "python2")]
        {
            let s = obj.cast_as::<PyBytes>(py)?.data(py);
            let path = local_bytes_to_path(s).map_pyerr(py)?;
            Ok(Self(
                path.to_str()
                    .ok_or_else(|| format_err!("{} is not a UTF-8 path", path.display()))
                    .map_pyerr(py)?
                    .into(),
            ))
        }
    }
}

impl TryFrom<PathBuf> for PyPath {
    type Error = anyhow::Error;

    fn try_from(path: PathBuf) -> Result<Self> {
        path.as_path().try_into()
    }
}

impl<'a> TryFrom<&'a Path> for PyPath {
    type Error = anyhow::Error;

    fn try_from(path: &'a Path) -> Result<Self> {
        Ok(Self(
            path.to_str()
                .ok_or_else(|| format_err!("{} is not a UTF-8 path", path.display()))?
                .into(),
        ))
    }
}

impl From<String> for PyPath {
    fn from(s: String) -> PyPath {
        Self(s)
    }
}

impl AsRef<Path> for PyPath {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}
