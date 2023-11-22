/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use cpython::*;
use cpython_ext::error::AnyhowResultExt;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::ExtractInner;
use cpython_ext::ExtractInnerRef;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use pathmatcher::AlwaysMatcher;
use pathmatcher::DifferenceMatcher;
use pathmatcher::DirectoryMatch;
use pathmatcher::DynMatcher;
use pathmatcher::GitignoreMatcher;
use pathmatcher::HintedMatcher;
use pathmatcher::Matcher;
use pathmatcher::NeverMatcher;
use pathmatcher::PatternKind;
use pathmatcher::TreeMatcher;
use pathmatcher::UnionMatcher;
use pathmatcher::XorMatcher;
use tracing::debug;
use types::RepoPath;
use types::RepoPathBuf;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "pathmatcher"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<gitignorematcher>(py)?;
    m.add_class::<treematcher>(py)?;
    m.add_class::<hintedmatcher>(py)?;
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
    data matcher: Arc<GitignoreMatcher>;

    def __new__(_cls, root: &PyPath, global_paths: Vec<PyPathBuf>, case_sensitive: bool) -> PyResult<gitignorematcher> {
        let global_paths: Vec<&Path> = global_paths.iter().map(PyPathBuf::as_path).collect();
        let matcher = GitignoreMatcher::new(root, global_paths, case_sensitive);
        Self::create_instance(py, Arc::new(matcher))
    }

    def match_relative(&self, path: &PyPath, is_dir: bool) -> PyResult<bool> {
        Ok(self.matcher(py).match_relative(path, is_dir))
    }

    def explain(&self, path: &PyPath, is_dir: bool) -> PyResult<String> {
        Ok(self.matcher(py).explain(path, is_dir))
    }
});

impl ExtractInnerRef for gitignorematcher {
    type Inner = Arc<GitignoreMatcher>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.matcher(py)
    }
}

py_class!(pub class treematcher |py| {
    data matcher: Arc<TreeMatcher>;

    def __new__(_cls, rules: Vec<String>, case_sensitive: bool) -> PyResult<Self> {
        let matcher = TreeMatcher::from_rules(rules.into_iter(), case_sensitive).map_pyerr(py)?;
        Self::create_instance(py, Arc::new(matcher))
    }

    def matches(&self, path: &PyPath) -> PyResult<bool> {
        Ok(self.matcher(py).matches(path.as_str()))
    }

    def match_recursive(&self, path: &PyPath) -> PyResult<Option<bool>> {
        if path.as_path().as_os_str().is_empty() {
            Ok(None)
        } else {
            Ok(self.matcher(py).match_recursive(path.as_str()))
        }
    }

    def matching_rule_indexes(&self, path: &PyPath) -> PyResult<Vec<usize>> {
        Ok(self.matcher(py).matching_rule_indexes(path.as_str()))
    }
});

impl ExtractInnerRef for treematcher {
    type Inner = Arc<TreeMatcher>;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.matcher(py)
    }
}

py_class!(pub class hintedmatcher |py| {
    data matcher: HintedMatcher;

    def __new__(_cls,
        patterns: Vec<String>,
        pattern_fset: Option<Vec<PyPathBuf>>,
        include: Vec<String>,
        include_fset: Option<Vec<PyPathBuf>>,
        exclude: Vec<String>,
        exclude_fset: Option<Vec<PyPathBuf>>,
        default_pattern_type: String,
        case_sensitive: bool,
        root: &PyPath,
        cwd: &PyPath,
    ) -> PyResult<Self> {
        fn into_repo_paths(paths: Option<Vec<PyPathBuf>>) -> Result<Option<Vec<RepoPathBuf>>> {
            paths
                .map(|paths| paths.into_iter().map(|p| p.to_repo_path_buf()).collect::<Result<Vec<_>>>())
                .transpose()
        }

        let matcher = pathmatcher::cli_matcher_with_filesets(
            &patterns,
            into_repo_paths(pattern_fset).map_pyerr(py)?.as_deref(),
            &include,
            into_repo_paths(include_fset).map_pyerr(py)?.as_deref(),
            &exclude,
            into_repo_paths(exclude_fset).map_pyerr(py)?.as_deref(),
            PatternKind::from_str(&default_pattern_type).map_pyerr(py)?,
            case_sensitive,
            root.as_path(),
            cwd.as_path(),
            &mut io::IO::main().map_pyerr(py)?.input(),
        ).map_pyerr(py)?;
        Self::create_instance(py, matcher)
    }

    def matches_file(&self, path: &PyPath) -> PyResult<bool> {
        let repo_path = path.to_repo_path().map_pyerr(py)?;
        self.matcher(py).matches_file(repo_path).map_pyerr(py)
    }

    def matches_directory(&self, path: &PyPath) -> PyResult<Option<bool>> {
        let repo_path = path.to_repo_path().map_pyerr(py)?;
        let directory_match = self.matcher(py).matches_directory(repo_path).map_pyerr(py)?;
        match directory_match {
            DirectoryMatch::Everything => Ok(Some(true)),
            DirectoryMatch::Nothing => Ok(Some(false)),
            DirectoryMatch::ShouldTraverse => Ok(None)
        }
    }

    def exact_files(&self) -> PyResult<Vec<PyPathBuf>> {
        Ok(self.matcher(py).exact_files().iter().map(|p| p.clone().into()).collect())
    }

    def always_matches(&self) -> PyResult<bool> {
        Ok(self.matcher(py).always_matches())
    }

    def never_matches(&self) -> PyResult<bool> {
        Ok(self.matcher(py).never_matches())
    }

    def all_recursive_paths(&self) -> PyResult<bool> {
        Ok(self.matcher(py).all_recursive_paths())
    }

    def warnings(&self) -> PyResult<Vec<String>> {
        Ok(self.matcher(py).warnings().to_vec())
    }
});

impl ExtractInnerRef for hintedmatcher {
    type Inner = DynMatcher;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner {
        self.matcher(py).matcher()
    }
}

fn normalize_glob(_py: Python, path: &str) -> PyResult<String> {
    Ok(pathmatcher::normalize_glob(path))
}

fn plain_to_glob(_py: Python, path: &str) -> PyResult<String> {
    Ok(pathmatcher::plain_to_glob(path))
}

fn expand_curly_brackets(_py: Python, pattern: &str) -> PyResult<Vec<String>> {
    Ok(pathmatcher::expand_curly_brackets(pattern))
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
    fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch> {
        matches_directory_impl(self.py, &self.py_matcher, path).into_anyhow_result()
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        matches_file_impl(self.py, &self.py_matcher, path).into_anyhow_result()
    }
}

pub struct ThreadPythonMatcher {
    py_matcher: PyObject,
}

impl ThreadPythonMatcher {
    pub fn new(py_matcher: PyObject) -> Self {
        ThreadPythonMatcher { py_matcher }
    }
}

impl Matcher for ThreadPythonMatcher {
    fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch> {
        let gil = Python::acquire_gil();
        matches_directory_impl(gil.python(), &self.py_matcher, path).into_anyhow_result()
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        let gil = Python::acquire_gil();
        matches_file_impl(gil.python(), &self.py_matcher, path).into_anyhow_result()
    }
}

fn matches_directory_impl(
    py: Python,
    py_matcher: &PyObject,
    path: &RepoPath,
) -> PyResult<DirectoryMatch> {
    let py_path = PyPathBuf::from(path);
    // PANICS! The interface in Rust doesn't expose exceptions. Unwrapping seems fine since
    // it crashes the rust stuff and returns a rust exception to Python.
    let py_value = py_matcher.call_method(py, "visitdir", (py_path,), None)?;

    let is_all = PyString::extract(py, &py_value)
        .and_then(|py_str| py_str.to_string(py).map(|s| s == "all"))
        .unwrap_or(false);
    let matches = if is_all {
        DirectoryMatch::Everything
    } else if py_value.is_true(py).unwrap() {
        DirectoryMatch::ShouldTraverse
    } else {
        DirectoryMatch::Nothing
    };
    Ok(matches)
}

fn matches_file_impl(py: Python, py_matcher: &PyObject, path: &RepoPath) -> PyResult<bool> {
    let py_path = PyPathBuf::from(path);
    // PANICS! The interface in Rust doesn't expose exceptions. Unwrapping seems fine since
    // it crashes the rust stuff and returns a rust exception to Python.
    let matches = py_matcher.call(py, (py_path,), None)?.is_true(py)?;
    Ok(matches)
}

/// Extracts a Rust matcher from a Python Object
/// When possible it converts it into a pure-Rust matcher.
pub fn extract_matcher(py: Python, matcher: PyObject) -> PyResult<Arc<dyn Matcher + Sync + Send>> {
    if let Ok(matcher) = treematcher::downcast_from(py, matcher.clone_ref(py)) {
        debug!("treematcher downcast");
        return Ok(matcher.extract_inner(py));
    }
    if let Ok(matcher) = gitignorematcher::downcast_from(py, matcher.clone_ref(py)) {
        debug!("gitignorematcher downcast");
        return Ok(matcher.extract_inner(py));
    }
    if let Ok(matcher) = hintedmatcher::downcast_from(py, matcher.clone_ref(py)) {
        debug!("hintedmatcher downcast");
        return Ok(matcher.extract_inner(py));
    }

    let py_type = matcher.get_type(py);
    let type_name = py_type.name(py);

    debug!(%type_name);

    if matches!(
        type_name.as_ref(),
        "treematcher" | "gitignorematcher" | "hintedmatcher"
    ) {
        return extract_matcher(py, matcher.getattr(py, "_matcher")?);
    }

    if type_name.as_ref() == "unionmatcher" {
        let py_matchers = matcher.getattr(py, "_matchers")?;
        let py_matchers = PyList::extract(py, &py_matchers)?;
        let mut matchers: Vec<Arc<dyn Matcher + Sync + Send>> = vec![];
        for matcher in py_matchers.iter(py) {
            matchers.push(extract_matcher(py, matcher)?);
        }

        return Ok(Arc::new(UnionMatcher::new(matchers)));
    }
    if type_name.as_ref() == "differencematcher" {
        let include = extract_matcher(py, matcher.getattr(py, "_m1")?)?;
        let exclude = extract_matcher(py, matcher.getattr(py, "_m2")?)?;
        return Ok(Arc::new(DifferenceMatcher::new(include, exclude)));
    }
    if type_name.as_ref() == "xormatcher" {
        let m1 = extract_matcher(py, matcher.getattr(py, "m1")?)?;
        let m2 = extract_matcher(py, matcher.getattr(py, "m2")?)?;
        return Ok(Arc::new(XorMatcher::new(m1, m2)));
    }
    if type_name.as_ref() == "alwaysmatcher" {
        return Ok(Arc::new(AlwaysMatcher::new()));
    }
    if type_name.as_ref() == "nevermatcher" {
        return Ok(Arc::new(NeverMatcher::new()));
    }

    Ok(Arc::new(ThreadPythonMatcher::new(matcher)))
}

pub fn extract_option_matcher(
    py: Python,
    matcher: Option<PyObject>,
) -> PyResult<Arc<dyn Matcher + Sync + Send>> {
    match matcher {
        None => Ok(Arc::new(AlwaysMatcher::new())),
        Some(m) => extract_matcher(py, m),
    }
}
