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
        let py = self.py;
        let py_path = PyBytes::new(py, path.as_byte_slice());
        // PANICS! The interface in Rust doesn't expose exceptions. Unwrapping seems fine since
        // it crashes the rust stuff and returns a rust exception to Python.
        let py_result = self
            .py_matcher
            .call_method(py, "visitdir", (py_path,), None)
            .unwrap();
        match PyBool::extract(py, &py_result) {
            Ok(py_bool) => match py_bool.is_true() {
                true => DirectoryMatch::ShouldTraverse,
                false => DirectoryMatch::Nothing,
            },
            Err(_) => {
                let py_string = PyString::extract(py, &py_result).unwrap();
                if py_string.to_string(py).unwrap() == "all" {
                    DirectoryMatch::Everything
                } else {
                    panic!("Unexpected value returned from matcher: {:?}", py_result);
                }
            }
        }
    }

    fn matches_file(&self, path: &RepoPath) -> bool {
        let py = self.py;
        let py_path = PyBytes::new(py, path.as_byte_slice());
        // PANICS! The interface in Rust doesn't expose exceptions. Unwrapping seems fine since
        // it crashes the rust stuff and returns a rust exception to Python.
        let py_result = self.py_matcher.call(py, (py_path,), None).unwrap();
        PyBool::extract(py, &py_result).unwrap().is_true()
    }
}
