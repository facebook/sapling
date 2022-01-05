/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Borrow;
use std::fmt;
use std::mem;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use cpython::*;
#[cfg(feature = "python2")]
use encoding::local_bytes_to_path;
#[cfg(feature = "python2")]
use encoding::path_to_local_bytes;
use thiserror::Error;
use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

#[cfg(feature = "python2")]
use crate::ResultPyErrExt;

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Default, Hash, Ord)]
pub struct PyPathBuf(String);

#[derive(Debug, Eq, PartialEq, PartialOrd, Hash, Ord)]
pub struct PyPath(str);

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0:?} is not a valid UTF-8 path")]
    NonUTF8Path(PathBuf),
}

impl PyPathBuf {
    pub fn as_pypath(&self) -> &PyPath {
        self
    }

    pub fn as_path(&self) -> &Path {
        self.0.as_ref()
    }

    pub fn to_path_buf(&self) -> PathBuf {
        Path::new(&self.0).to_path_buf()
    }

    pub fn to_repo_path_buf(self) -> Result<RepoPathBuf> {
        Ok(RepoPathBuf::from_string(self.0)?)
    }

    pub fn to_repo_path<'a>(&'a self) -> Result<&'a RepoPath> {
        Ok(RepoPath::from_str(&self.0)?)
    }

    pub fn into_utf8_bytes(self) -> Vec<u8> {
        self.0.into()
    }

    pub fn as_utf8_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub fn from_string(s: String) -> Self {
        Self(s)
    }

    pub fn from_utf8_bytes(utf8_bytes: Vec<u8>) -> Result<Self> {
        Ok(Self(String::from_utf8(utf8_bytes)?))
    }
}

impl ToPyObject for PyPathBuf {
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

impl<'source> FromPyObject<'source> for PyPathBuf {
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
                    .ok_or_else(|| Error::NonUTF8Path(path.to_path_buf()))
                    .map_pyerr(py)?
                    .into(),
            ))
        }
    }
}

impl TryFrom<PathBuf> for PyPathBuf {
    type Error = anyhow::Error;

    fn try_from(path: PathBuf) -> Result<Self> {
        path.as_path().try_into()
    }
}

impl<'a> TryFrom<&'a Path> for PyPathBuf {
    type Error = anyhow::Error;

    fn try_from(path: &'a Path) -> Result<Self> {
        Ok(Self(
            path.to_str()
                .ok_or_else(|| Error::NonUTF8Path(path.to_path_buf()))?
                .into(),
        ))
    }
}

impl From<String> for PyPathBuf {
    fn from(s: String) -> PyPathBuf {
        Self(s)
    }
}

impl<'a> From<&'a RepoPath> for PyPathBuf {
    fn from(repo_path: &'a RepoPath) -> PyPathBuf {
        PyPathBuf(repo_path.as_str().to_owned())
    }
}

impl From<RepoPathBuf> for PyPathBuf {
    fn from(repo_path_buf: RepoPathBuf) -> PyPathBuf {
        PyPathBuf(repo_path_buf.into_string())
    }
}

impl From<PathComponentBuf> for PyPathBuf {
    fn from(path_component_buf: PathComponentBuf) -> PyPathBuf {
        PyPathBuf(path_component_buf.into_string())
    }
}

impl fmt::Display for PyPathBuf {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&*self.0, formatter)
    }
}

impl From<PyPathBuf> for String {
    fn from(path: PyPathBuf) -> String {
        path.0
    }
}

impl Deref for PyPathBuf {
    type Target = PyPath;
    fn deref(&self) -> &Self::Target {
        unsafe { mem::transmute(&*self.0) }
    }
}

impl AsRef<PyPath> for PyPathBuf {
    fn as_ref(&self) -> &PyPath {
        self
    }
}

impl PyPath {
    pub fn from_str(s: &str) -> &Self {
        unsafe { mem::transmute(s) }
    }

    pub fn as_path(&self) -> &Path {
        self.0.as_ref()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_utf8_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub fn to_path_buf(&self) -> PathBuf {
        Path::new(&self.0).to_path_buf()
    }

    pub fn to_repo_path(&self) -> Result<&RepoPath> {
        Ok(RepoPath::from_str(&self.0)?)
    }

    pub fn to_repo_path_buf(&self) -> Result<RepoPathBuf> {
        Ok(RepoPathBuf::from_string(self.0.to_string())?)
    }

    pub fn into_utf8_bytes(&self) -> Vec<u8> {
        self.0.into()
    }
}

impl ToOwned for PyPath {
    type Owned = PyPathBuf;

    fn to_owned(&self) -> Self::Owned {
        PyPathBuf(self.0.to_string())
    }
}

impl Borrow<PyPath> for PyPathBuf {
    fn borrow(&self) -> &PyPath {
        self
    }
}

impl RefFromPyObject for PyPath {
    fn with_extracted<F, R>(py: Python, obj: &PyObject, f: F) -> PyResult<R>
    where
        F: FnOnce(&PyPath) -> R,
    {
        #[cfg(feature = "python3")]
        {
            let s = obj.cast_as::<PyUnicode>(py)?.to_string(py)?;
            Ok(f(PyPath::from_str(s.as_ref())))
        }

        #[cfg(feature = "python2")]
        {
            let s = obj.cast_as::<PyBytes>(py)?.data(py);
            let path = local_bytes_to_path(s).map_pyerr(py)?;
            let py_path = PyPath::from_str(
                path.to_str()
                    .ok_or_else(|| Error::NonUTF8Path(path.to_path_buf()))
                    .map_pyerr(py)?,
            );
            Ok(f(py_path))
        }
    }
}

impl AsRef<PyPath> for PyPath {
    fn as_ref(&self) -> &PyPath {
        self
    }
}

impl AsRef<Path> for PyPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl fmt::Display for PyPath {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}
