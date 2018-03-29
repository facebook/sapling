#![allow(non_camel_case_types)]

#[macro_use]
extern crate cpython;
extern crate pathencoding;
extern crate pathmatcher;

use cpython::{PyBytes, PyErr, PyResult, Python};
use cpython::exc::RuntimeError;
use pathencoding::local_bytes_to_path;
use pathmatcher::GitignoreMatcher;

fn encoding_error(py: Python) -> PyErr {
    PyErr::new::<RuntimeError, _>(py, "invalid encoding")
}

py_module_initializer!(matcher, initmatcher, PyInit_matcher, |py, m| {
    m.add_class::<gitignorematcher>(py)?;
    Ok(())
});

py_class!(class gitignorematcher |py| {
    data matcher: GitignoreMatcher;

    def __new__(_cls, path: &PyBytes) -> PyResult<gitignorematcher> {
        let path = local_bytes_to_path(path.data(py)).map_err(|_|encoding_error(py))?;
        let matcher = GitignoreMatcher::new(&path);
        gitignorematcher::create_instance(py, matcher)
    }

    def match_relative(&self, path: &PyBytes, is_dir: bool) -> PyResult<bool> {
        let path = local_bytes_to_path(path.data(py)).map_err(|_|encoding_error(py))?;
        Ok(self.matcher(py).match_relative(&path, is_dir))
    }
});
