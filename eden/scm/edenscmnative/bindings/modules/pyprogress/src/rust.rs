/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This module contains Rust wrappers around Mercurial's Python `progress`
//! module, allowing Rust code to create and use Python progress bars. In
//! typical usage, the Rust code will interact with these types as trait
//! objects, abstracting away the fact that these are implemented in Python.

use std::sync::Arc;

use anyhow::{Context, Result};
use cpython::*;

use cpython_ext::{AnyhowResultExt, PyNone};
use progress::{ProgressBar, ProgressFactory, ProgressSpinner, Unit};

const HG_PROGRESS_MOD: &str = "edenscm.mercurial.progress";
const HG_UTIL_MOD: &str = "edenscm.mercurial.util";

/// Rust wrapper around Mercurial's Python `progress` module, allowing
/// otherwise pure Rust code to create Python progress bars and spinners.
pub struct PyProgressFactory {
    progmod: PyModule,
    bytecount: PyObject,
    ui: PyObject,
}

impl PyProgressFactory {
    pub fn new(py: Python, ui: PyObject) -> PyResult<Self> {
        Ok(Self {
            progmod: py.import(HG_PROGRESS_MOD)?,
            bytecount: py.import(HG_UTIL_MOD)?.get(py, "bytecount")?,
            ui,
        })
    }

    pub fn arc(py: Python, ui: PyObject) -> PyResult<Arc<dyn ProgressFactory>> {
        Ok(Arc::new(Self::new(py, ui)?))
    }
}

impl ProgressFactory for PyProgressFactory {
    fn bar(
        &self,
        message: &str,
        total: Option<u64>,
        unit: Unit<'_>,
    ) -> Result<Box<dyn ProgressBar>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let (unit, formatfunc) = match unit {
            Unit::None => (None, None),
            Unit::Bytes => (Some("bytes"), Some(&self.bytecount)),
            Unit::Named(name) => (Some(name), None),
        };

        let bar = PyProgressBar::bar(
            py,
            &self.progmod,
            &self.ui,
            message,
            total,
            unit,
            formatfunc,
        )
        .into_anyhow_result()
        .context("Failed to initialize Python progress bar")?;

        Ok(Box::new(bar))
    }

    fn spinner(&self, message: &str) -> Result<Box<dyn ProgressSpinner>> {
        let gil = Python::acquire_gil();
        let py = gil.python();

        let spinner = PyProgressBar::spinner(py, &self.progmod, &self.ui, message)
            .into_anyhow_result()
            .context("Failed to initialize Python progress spinner")?;

        Ok(Box::new(spinner))
    }
}

/// A Mercurial progress bar or spinner.
///
/// The underlying Python representation is the same for both progress bars
/// and spinners, so this single type implements both the `ProgressBar` and
/// `ProgressSpinner` traits. In practice, Rust code will interact with this
/// type as a trait object, and will therefore not be able to call unsupported
/// methods (e.g., querying the position or total length of a spinner).
struct PyProgressBar {
    bar: PyObject,
}

impl PyProgressBar {
    /// Create and start a new progress spinner.
    fn bar(
        py: Python,
        progmod: &PyModule,
        ui: &PyObject,
        message: &str,
        total: Option<u64>,
        unit: Option<&str>,
        formatfunc: Option<&PyObject>,
    ) -> PyResult<Self> {
        let kwargs = PyDict::new(py);
        kwargs.set_item(py, "total", total)?;
        kwargs.set_item(py, "unit", unit)?;
        kwargs.set_item(py, "formatfunc", formatfunc)?;

        // Manually enter the context manager.
        let bar = progmod
            .call(py, "bar", (ui, message), Some(&kwargs))?
            .call_method(py, "__enter__", NoArgs, None)?;

        Ok(Self { bar })
    }

    /// Create and start a new progress spinner.
    fn spinner(py: Python, progmod: &PyModule, ui: &PyObject, message: &str) -> PyResult<Self> {
        // Manually enter the context manager.
        let bar = progmod
            .call(py, "spinner", (ui, message), None)?
            .call_method(py, "__enter__", NoArgs, None)?;

        Ok(Self { bar })
    }

    fn position_py(&self, py: Python) -> PyResult<u64> {
        self.bar.getattr(py, "value")?.extract(py)
    }

    fn total_py(&self, py: Python) -> PyResult<Option<u64>> {
        self.bar.getattr(py, "_total")?.extract(py)
    }

    fn set_py(&self, py: Python, pos: u64) -> PyResult<()> {
        self.bar.setattr(py, "value", pos)
    }

    fn set_total_py(&self, py: Python, total: Option<u64>) -> PyResult<()> {
        self.bar.setattr(py, "_total", total)
    }

    fn increment_py(&self, py: Python, delta: u64) -> PyResult<()> {
        let pos = self.position_py(py)?;
        self.set_py(py, pos + delta)
    }

    fn set_message_py(&self, py: Python, message: &str) -> PyResult<()> {
        self.bar.setattr(py, "_topic", message)
    }
}

impl ProgressBar for PyProgressBar {
    fn position(&self) -> Result<u64> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        self.position_py(py).into_anyhow_result()
    }

    fn total(&self) -> Result<Option<u64>> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        self.total_py(py).into_anyhow_result()
    }

    fn set(&self, pos: u64) -> Result<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        self.set_py(py, pos).into_anyhow_result()
    }

    fn set_total(&self, total: Option<u64>) -> Result<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        self.set_total_py(py, total).into_anyhow_result()
    }

    fn increment(&self, delta: u64) -> Result<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        self.increment_py(py, delta).into_anyhow_result()
    }

    fn set_message(&self, message: &str) -> Result<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        self.set_message_py(py, message).into_anyhow_result()
    }
}

impl ProgressSpinner for PyProgressBar {
    fn set_message(&self, message: &str) -> Result<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        self.set_message_py(py, message).into_anyhow_result()
    }
}

impl Drop for PyProgressBar {
    fn drop(&mut self) {
        let gil = Python::acquire_gil();
        let py = gil.python();

        // Exit context manager on drop.
        let _ = self
            .bar
            .call_method(py, "__exit__", (PyNone, PyNone, PyNone), None)
            .expect("Failed to call __exit__ while dropping progress bar");
    }
}
