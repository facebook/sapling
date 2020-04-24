/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::path::Path;

use cpython::*;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::{PyPath, PyPathBuf, Str};

use pathmatcher::{DirectoryMatch, GitignoreMatcher, Matcher, TreeMatcher};
use types::RepoPath;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "pathmatcher"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<gitignorematcher>(py)?;
    m.add_class::<treematcher>(py)?;
    m.add(py, "normalizeglob", py_fn!(py, normalize_glob(path: &str)))?;
    m.add(py, "plaintoglob", py_fn!(py, plain_to_glob(path: &str)))?;
    m.add(
        py,
        "expandcurlybrackets",
        py_fn!(py, expand_curly_brackets(path: &str)),
    )?;
    Ok(m)
}

py_class!(class gitignorematcher |py| {
    data matcher: GitignoreMatcher;

    def __new__(_cls, root: &PyPath, global_paths: Vec<PyPathBuf>) -> PyResult<gitignorematcher> {
        let global_paths: Vec<&Path> = global_paths.iter().map(PyPathBuf::as_path).collect();
        let matcher = GitignoreMatcher::new(root, global_paths);
        gitignorematcher::create_instance(py, matcher)
    }

    def match_relative(&self, path: &PyPath, is_dir: bool) -> PyResult<bool> {
        Ok(self.matcher(py).match_relative(path, is_dir))
    }

    def explain(&self, path: &PyPath, is_dir: bool) -> PyResult<Str> {
        Ok(self.matcher(py).explain(path, is_dir).into())
    }
});

py_class!(class treematcher |py| {
    data matcher: TreeMatcher;

    def __new__(_cls, rules: Vec<String>) -> PyResult<Self> {
        let matcher = TreeMatcher::from_rules(rules.into_iter()).map_pyerr(py)?;
        Self::create_instance(py, matcher)
    }

    def matches(&self, path: &PyPath) -> PyResult<bool> {
        Ok(self.matcher(py).matches(path))
    }

    def match_recursive(&self, path: &PyPath) -> PyResult<Option<bool>> {
        if path.as_path().as_os_str().is_empty() {
            Ok(None)
        } else {
            Ok(self.matcher(py).match_recursive(path))
        }
    }
});

fn normalize_glob(_py: Python, path: &str) -> PyResult<Str> {
    Ok(pathmatcher::normalize_glob(path).into())
}

fn plain_to_glob(_py: Python, path: &str) -> PyResult<Str> {
    Ok(pathmatcher::plain_to_glob(path).into())
}

fn expand_curly_brackets(_py: Python, pattern: &str) -> PyResult<Vec<Str>> {
    Ok(pathmatcher::expand_curly_brackets(pattern)
        .into_iter()
        .map(|s| s.into())
        .collect())
}

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

impl Clone for UnsafePythonMatcher {
    fn clone(&self) -> Self {
        let gil = Python::acquire_gil();
        UnsafePythonMatcher {
            py_matcher: self.py_matcher.clone_ref(gil.python()),
        }
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
    let py_path = PyPathBuf::from(path);
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
    let py_path = PyPathBuf::from(path);
    // PANICS! The interface in Rust doesn't expose exceptions. Unwrapping seems fine since
    // it crashes the rust stuff and returns a rust exception to Python.
    py_matcher
        .call(py, (py_path,), None)
        .unwrap()
        .is_true(py)
        .unwrap()
}
