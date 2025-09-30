/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::convert::Serde;
use termwiz::surface::Change as NativeChange;

py_class!(pub class Change |py| {
    data inner: NativeChange;

    def __new__(_cls, change: Serde<NativeChange>) -> PyResult<Self> {
        Self::create_instance(py, change.0)
    }

    def __repr__(&self) -> PyResult<String> {
        let repr = format!("<Change {:?}>", self.inner(py));
        Ok(repr)
    }

    def is_text(&self) -> PyResult<bool> {
        Ok(self.inner(py).is_text())
    }

    def text(&self) -> PyResult<String> {
        Ok(self.inner(py).text().to_string())
    }
});

impl Change {
    pub fn to_native(&self, py: Python) -> NativeChange {
        self.inner(py).clone()
    }
}
