/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use std::any::Any;
use std::cell::RefCell;
use std::fmt::Debug;

/// pycell allows to put arbitrary rust data and pass it between different rust functions through python code
/// This allows to avoid using bincode or writing wrapper types for some basic use cases
py_class!(pub class pycell |py| {
    data inner: RefCell<Option<Box<dyn Any + Sync + Send + 'static>>>;
    data fmt: Box<dyn (Fn(&(dyn Any)) -> String) + Send + Sync>;

    def __str__(&self) -> PyResult<String> {
        let str = self.inner(py).borrow().as_ref().map(|inner| {
                let fmt = self.fmt(py);
                fmt(inner)
        });
        let str = str.unwrap_or_else(||"<None>".to_string());
        Ok(str)
    }
});

impl pycell {
    pub fn new<T: Debug + Sync + Send + 'static + Sized>(py: Python, data: T) -> PyResult<Self> {
        let inner = Box::new(data) as Box<dyn Any + Sync + Send + 'static>;
        let fmt = |obj: &(dyn Any)| {
            let obj = obj.downcast_ref::<T>().unwrap(); // does not fail
            format!("{:?}", obj)
        };
        Self::create_instance(py, RefCell::new(Some(inner)), Box::new(fmt))
    }

    pub fn take<T: Sync + Send + 'static + Sized>(&self, py: Python) -> Option<Box<T>> {
        match self.inner(py).borrow_mut().take() {
            Some(x) => x.downcast::<T>().ok(),
            None => None,
        }
    }
}
