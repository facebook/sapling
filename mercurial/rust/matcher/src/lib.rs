#![allow(non_camel_case_types)]

#[macro_use]
extern crate cpython;
extern crate encoding;
extern crate pathmatcher;

use std::path::Path;
use cpython::{PyBytes, PyErr, PyResult, Python};
use encoding::local_bytes_to_path;
use pathmatcher::GitignoreMatcher;

fn encoding_error(py: Python) -> PyErr {
    PyErr::new::<cpython::exc::RuntimeError, _>(py, "invalid encoding")
}

py_module_initializer!(matcher, initmatcher, PyInit_matcher, |py, m| {
    m.add_class::<gitignorematcher>(py)?;
    Ok(())
});

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
});
