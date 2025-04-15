/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cell::RefCell;

use ::serde::Serialize;
use anyhow::Result;
use cpython::*;

use crate::ResultPyErrExt as _;
use crate::ser::to_object;

// Exposes a Rust iterator as a Python iterator.
// This allows to avoid using bincode or writing wrapper types for some basic use cases
py_class!(pub class PyIter |py| {
    data next_func: RefCell<Box<dyn FnMut(Python) -> PyResult<Option<PyObject>> + Send>>;

    def __next__(&self) -> PyResult<Option<PyObject>> {
        let func = self.next_func(py);
        let mut func = func.borrow_mut();
        (func)(py)
    }

    def __iter__(&self) -> PyResult<PyIter> {
        Ok(self.clone_ref(py))
    }
});

impl PyIter {
    /// Wraps a Rust iterator so it works as a Python iterator.
    /// The value produced by `iter` will be serialized to PyObject using serde.
    pub fn new<T: Serialize + 'static>(
        py: Python,
        iter: impl Iterator<Item = Result<T>> + Send + 'static,
    ) -> PyResult<Self> {
        Self::new_custom(py, iter, |py, v| to_object(py, &v))
    }

    /// Wraps a Rust iterator so it works as a Python iterator.
    /// The `convert_fn` is used to convert a Rust item to a Python object.
    pub fn new_custom<T: 'static>(
        py: Python,
        mut iter: impl Iterator<Item = Result<T>> + Send + 'static,
        convert_fn: for<'a> fn(Python<'a>, T) -> PyResult<PyObject>,
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
                Some(v) => Ok(Some(convert_fn(py, v)?)),
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

        let list = vec![5, 10, 15];
        let iter1 = list.clone().into_iter().map(Ok::<_, anyhow::Error>);
        let py_iter1 = PyIter::new(py, iter1).unwrap();

        struct S(usize);
        let iter2 = list.into_iter().map(|v| Ok::<_, anyhow::Error>(S(v)));
        let py_iter2 =
            PyIter::new_custom(py, iter2, |py, v| Ok(v.0.to_py_object(py).into_object())).unwrap();

        for py_iter in [py_iter1, py_iter2] {
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
}
