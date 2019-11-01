// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![allow(non_camel_case_types)]

use std::path::Path;

use cpython::*;
use cpython_ext::Bytes;

use encoding::local_bytes_to_path;
use pathmatcher::{DirectoryMatch, GitignoreMatcher, Matcher};
use types::RepoPath;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "pathmatcher"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<gitignorematcher>(py)?;
    Ok(m)
}

fn encoding_error(py: Python) -> PyErr {
    PyErr::new::<cpython::exc::RuntimeError, _>(py, "invalid encoding")
}

py_class!(class gitignorematcher |py| {
    data matcher: GitignoreMatcher;

    def __new__(_cls, root: &PyBytes, global_paths: Vec<PyBytes>) -> PyResult<gitignorematcher> {
        let root = local_bytes_to_path(root.data(py)).map_err(|_|encoding_error(py))?;
        let global_paths : Result<Vec<_>, _> = global_paths.iter()
            .map(|path| local_bytes_to_path(path.data(py))).collect();
        let global_paths = global_paths.map_err(|_|encoding_error(py))?;
        let global_paths: Vec<&Path> = global_paths.iter().map(AsRef::as_ref).collect();
        let matcher = GitignoreMatcher::new(&root, global_paths);
        gitignorematcher::create_instance(py, matcher)
    }

    def match_relative(&self, path: &PyBytes, is_dir: bool) -> PyResult<bool> {
        let path = local_bytes_to_path(path.data(py)).map_err(|_|encoding_error(py))?;
        Ok(self.matcher(py).match_relative(&path, is_dir))
    }

    def explain(&self, path: &PyBytes, is_dir: bool) -> PyResult<Bytes> {
        let path = local_bytes_to_path(path.data(py)).map_err(|_|encoding_error(py))?;
        Ok(self.matcher(py).explain(&path, is_dir).into())
    }
});

pub struct PythonMatcher<'a> {
    py: Python<'a>,
    py_matcher: PyObject,
}

impl<'a> PythonMatcher<'a> {
    pub fn new(py: Python<'a>, py_matcher: PyObject) -> Self {
        PythonMatcher { py, py_matcher }
    }
}

impl<'a> Matcher for PythonMatcher<'a> {
    fn matches_directory(&self, path: &RepoPath) -> DirectoryMatch {
        matches_directory_impl(self.py, &self.py_matcher, &path)
    }

    fn matches_file(&self, path: &RepoPath) -> bool {
        matches_file_impl(self.py, &self.py_matcher, &path)
    }
}

// Matcher which does not store py. Should only be used when py cannot be stored in PythonMatcher
// struct and it is known that the GIL is acquired when calling matcher methods.
// Otherwise use PythonMatcher struct above
pub struct UnsafePythonMatcher {
    py_matcher: PyObject,
}

impl UnsafePythonMatcher {
    pub fn new(py_matcher: PyObject) -> Self {
        UnsafePythonMatcher { py_matcher }
    }
}

impl<'a> Matcher for UnsafePythonMatcher {
    fn matches_directory(&self, path: &RepoPath) -> DirectoryMatch {
        let assumed_py = unsafe { Python::assume_gil_acquired() };
        matches_directory_impl(assumed_py, &self.py_matcher, &path)
    }

    fn matches_file(&self, path: &RepoPath) -> bool {
        let assumed_py = unsafe { Python::assume_gil_acquired() };
        matches_file_impl(assumed_py, &self.py_matcher, &path)
    }
}

fn matches_directory_impl(py: Python, py_matcher: &PyObject, path: &RepoPath) -> DirectoryMatch {
    let py_path = PyBytes::new(py, path.as_byte_slice());
    // PANICS! The interface in Rust doesn't expose exceptions. Unwrapping seems fine since
    // it crashes the rust stuff and returns a rust exception to Python.
    let py_value = py_matcher
        .call_method(py, "visitdir", (py_path,), None)
        .unwrap();

    let is_all = PyString::extract(py, &py_value)
        .and_then(|py_str| py_str.to_string(py).map(|s| s == "all"))
        .unwrap_or(false);
    if is_all {
        DirectoryMatch::Everything
    } else {
        if py_value.is_true(py).unwrap() {
            DirectoryMatch::ShouldTraverse
        } else {
            DirectoryMatch::Nothing
        }
    }
}

fn matches_file_impl(py: Python, py_matcher: &PyObject, path: &RepoPath) -> bool {
    let py_path = PyBytes::new(py, path.as_byte_slice());
    // PANICS! The interface in Rust doesn't expose exceptions. Unwrapping seems fine since
    // it crashes the rust stuff and returns a rust exception to Python.
    py_matcher
        .call(py, (py_path,), None)
        .unwrap()
        .is_true(py)
        .unwrap()
}
