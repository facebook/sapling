/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_local_definitions)]

use cpython::*;
use cpython_ext::ResultPyErrExt;

mod change;
mod input_event;
mod surface;
mod terminal;

pub use change::Change;
pub use input_event::InputEventSerde;
pub use surface::Surface;
pub use terminal::BufferedTerminal;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "termwiz"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<BufferedTerminal>(py)?;
    m.add_class::<Change>(py)?;
    m.add_class::<Surface>(py)?;
    Ok(m)
}

pub(crate) trait MapTermwizError<T> {
    fn pyerr(self, py: Python) -> PyResult<T>;
}

impl<T> MapTermwizError<T> for termwiz::Result<T> {
    fn pyerr(self, py: Python) -> PyResult<T> {
        self.map_err(anyhow::Error::from).map_pyerr(py)
    }
}
