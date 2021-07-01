/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::convert::Serde;
use cpython::*;
use serde::Serialize;
use std::any::Any;
use std::cell::RefCell;
use std::fmt::Debug;

/// pycell allows to put arbitrary rust data and pass it between different rust functions through python code
/// This allows to avoid using bincode or writing wrapper types for some basic use cases
py_class!(pub class pycell |py| {
    data inner: RefCell<Option<Box<dyn Any + Sync + Send + 'static>>>;
    data fmt: Box<dyn (Fn(&(dyn Any)) -> String) + Send + Sync>;
    data pyobject: Box<dyn (Fn(&(dyn Any), Python) -> PyObject) + Send + Sync>;

    def __str__(&self) -> PyResult<String> {
        let str = self.inner(py).borrow().as_ref().map(|inner| {
                let fmt = self.fmt(py);
                fmt(inner)
        });
        let str = str.unwrap_or_else(||"<None>".to_string());
        Ok(str)
    }

    def export(&self) -> PyResult<Option<PyObject>> {
        let pyobj = self.inner(py).borrow().as_ref().map(|inner| {
            let pyobject = self.pyobject(py);
            pyobject(inner, py)
        });
        Ok(pyobj)
    }
});

impl pycell {
    pub fn new<T: Serialize + Debug + Sync + Send + 'static + Sized>(
        py: Python,
        data: T,
    ) -> PyResult<Self> {
        let inner = Box::new(data) as Box<dyn Any + Sync + Send + 'static>;
        let fmt = |obj: &(dyn Any)| {
            let obj = obj.downcast_ref::<T>().unwrap(); // does not fail
            format!("{:?}", obj)
        };
        let pyobject = |obj: &(dyn Any), py: Python| {
            let obj = obj.downcast_ref::<T>().unwrap(); // does not fail
            Serde(obj).to_py_object(py)
        };
        Self::create_instance(
            py,
            RefCell::new(Some(inner)),
            Box::new(fmt),
            Box::new(pyobject),
        )
    }

    pub fn take<T: Sync + Send + 'static + Sized>(&self, py: Python) -> Option<Box<T>> {
        match self.inner(py).borrow_mut().take() {
            Some(x) => x.downcast::<T>().ok(),
            None => None,
        }
    }
}
