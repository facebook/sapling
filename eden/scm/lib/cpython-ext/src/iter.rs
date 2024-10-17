/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;

use ::serde::Serialize;
use anyhow::Result;
use cpython::*;

use crate::ser::to_object;
use crate::ResultPyErrExt as _;

// Exposes a Rust iterator as a Python iterator.
// This allows to avoid using bincode or writing wrapper types for some basic use cases
py_class!(pub class PyIter |py| {
    data next_func: RefCell<Box<dyn FnMut(Python) -> PyResult<Option<PyObject>> + Send>>;

    def __next__(&self) -> PyResult<Option<PyObject>> {
        let func = self.next_func(py);
        let mut func = func.borrow_mut();
        (func)(py)
    }

});

impl PyIter {
    /// Wraps a Rust iterator so it works as a Python iterator.
    pub fn new<T: Serialize>(
        py: Python,
        mut iter: impl Iterator<Item = Result<T>> + Send + 'static,
    ) -> PyResult<Self> {
        let mut end = false;
        let next_func = move |py: Python| -> PyResult<Option<PyObject>> {
            if end {
                return Ok(None);
            }
            let next = py.allow_threads(|| iter.next());
            match next.transpose().map_pyerr(py)? {
                None => {
                    end = true;
                    Ok(None)
                }
                Some(v) => Ok(Some(to_object(py, &v)?)),
            }
        };

        Self::create_instance(py, RefCell::new(Box::new(next_func)))
    }
}

#[cfg(test)]
mod tests {
    use cpython::*;

    use super::*;

    #[test]
    fn test_py_iter() {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let iter = vec![5, 10, 15]
            .into_iter()
            .map(|v| Ok::<_, anyhow::Error>(v));
        let py_iter = PyIter::new(py, iter).unwrap();

        let item1 = py_iter.__next__(py).unwrap();
        let item2 = py_iter.__next__(py).unwrap();
        let item3 = py_iter.__next__(py).unwrap();
        let item4 = py_iter.__next__(py).unwrap();
        let item5 = py_iter.__next__(py).unwrap();

        let to_str = |v: Option<PyObject>| -> String {
            match v {
                None => "None".to_owned(),
                Some(v) => v.str(py).unwrap().to_string_lossy(py).into_owned(),
            }
        };

        assert_eq!(to_str(item1), "5");
        assert_eq!(to_str(item2), "10");
        assert_eq!(to_str(item3), "15");
        assert_eq!(to_str(item4), "None");
        assert_eq!(to_str(item5), "None");
    }
}
