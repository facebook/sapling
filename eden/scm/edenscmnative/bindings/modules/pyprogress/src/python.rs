/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This module contains Python wrappers around types that implement the
//! `ProgressBar` and `ProgressSpinner` traits, abstracting away the details
//! of the underlying progress bar implementation.

use std::cell::RefCell;

use cpython::*;

use cpython_ext::{PyNone, ResultPyErrExt};
use progress::{ProgressBar, ProgressFactory, ProgressSpinner, Unit};

use crate::rust::PyProgressFactory;

pub struct SpinnerParams {
    message: String,
}

pub struct BarParams {
    message: String,
    total: Option<u64>,
    unit: Option<String>,
}

/// Python code interacts with progress bars as context managers, whereas the
/// Rust API treats them as RAII guards. This enum is a shim to make sure that
/// we don't create (and therefore start) the progress bar until the we enter
/// the context manager.
pub enum State<F, P, T> {
    NotStarted { factory: F, params: P },
    Running(T),
    Done,
}

impl<F, P, T> State<F, P, T> {
    /// Get the underlying progress bar/spinner if it is currently running.
    fn running(&self) -> Option<&T> {
        match self {
            State::Running(inner) => Some(inner),
            _ => None,
        }
    }
}

py_class!(pub class spinner |py| {
    data state: RefCell<State<PyProgressFactory, SpinnerParams, Box<dyn ProgressSpinner>>>;

    def __new__(_cls, ui: PyObject, message: String) -> PyResult<spinner> {
        // XXX: This is hardcoded to use a PyProgressFactory because there isn't
        // presently a way for Python code to pass in a ProgressFactory. This
        // means that we are calling from Python -> Rust -> Python, which seems
        // somewhat pointless, but is still useful for onboarding Python code
        // onto this interface.
        //
        // TODO(kulshrax): Make the Python progress bindings accept a generic
        // ProgressFactory so that alternate progress bars can be used.
        let state = State::NotStarted {
            factory: PyProgressFactory::new(py, ui)?,
            params: SpinnerParams { message },
        };
        spinner::create_instance(py, RefCell::new(state))
    }

    def __enter__(&self) -> PyResult<spinner> {
        let mut state = self.state(py).borrow_mut();

        *state = match &*state {
            State::NotStarted { factory, params } => {
                // Start up and display the spinner.
                let spinner = factory.spinner(&params.message).map_pyerr(py)?;
                State::Running(spinner)
            }
            _ => {
                // Spinner is either already running or has already finished, so
                // just do nothing. (This should never happen, since it means
                // that the Python code has entered the context manager twice.)
                return Ok(self.clone_ref(py));
            }
        };

        Ok(self.clone_ref(py))
    }

    def __exit__(
        &self,
        _type: PyObject,
        _value: PyObject,
        _traceback: PyObject
    ) -> PyResult<PyNone> {
        // Drop the spinner to make it stop. Don't worry about exceptions here
        // since the progress spinner doesn't care about exceptions; as long
        // as we return a falsy value, the Python intepreter will continue to
        // progagate the exception normally.
        *self.state(py).borrow_mut() = State::Done;
        Ok(PyNone)
    }

    def set_message(&self, message: &str) -> PyResult<PyNone> {
        let state = self.state(py).borrow();
        if let Some(spinner) = state.running() {
            spinner.set_message(message).map_pyerr(py)?;
        }
        Ok(PyNone)
    }
});

py_class!(pub class bar |py| {
    data state: RefCell<State<PyProgressFactory, BarParams, Box<dyn ProgressBar>>>;

    def __new__(
        _cls,
        ui: PyObject,
        message: String,
        total: Option<u64> = None,
        unit: Option<String> = None
    ) -> PyResult<bar> {
        // XXX: This is hardcoded to use a PyProgressFactory because there isn't
        // presently a way for Python code to pass in a ProgressFactory. This
        // means that we are calling from Python -> Rust -> Python, which seems
        // somewhat pointless, but is still useful for onboarding Python code
        // onto this interface.
        //
        // TODO(kulshrax): Make the Python progress bindings accept a generic
        // ProgressFactory so that alternate progress bars can be used.
        let factory = PyProgressFactory::new(py, ui)?;

        let params = BarParams { message, total, unit };
        let state = State::NotStarted { factory, params };

        bar::create_instance(py, RefCell::new(state))
    }

    def __enter__(&self) -> PyResult<bar> {
        let mut state = self.state(py).borrow_mut();

        *state = match &*state {
            State::NotStarted { factory, params } => {
                let BarParams { message, total, unit } = params;

                // Start up and display the progress bar.
                let bar = factory.bar(
                    &message,
                    *total,
                    unit.as_deref().map(Unit::from).unwrap_or_default()
                ).map_pyerr(py)?;

                State::Running(bar)
            }
            _ => {
                // Progress bar is either already running or has already finished,
                // so just do nothing. (This should never happen, since it means
                // that the Python code has entered the context manager twice.)
                return Ok(self.clone_ref(py));
            }
        };

        Ok(self.clone_ref(py))
    }

    def __exit__(
        &self,
        _type: PyObject,
        _value: PyObject,
        _traceback: PyObject
    ) -> PyResult<PyNone> {
        // Drop the progress bar to make it stop. Don't worry about exceptions
        // here since the progress bar doesn't care about exceptions; as long
        // as we return a falsy value, the Python intepreter will continue to
        // progagate the exception normally.
        *self.state(py).borrow_mut() = State::Done;
        Ok(PyNone)
    }

    def position(&self) -> PyResult<Option<u64>> {
        let state = self.state(py).borrow();
        if let Some(bar) = state.running() {
            Ok(Some(bar.position().map_pyerr(py)?))
        } else {
            Ok(None)
        }
    }

    def total(&self) -> PyResult<Option<u64>> {
        let state = self.state(py).borrow();
        if let Some(bar) = state.running() {
            Ok(bar.total().map_pyerr(py)?)
        } else {
            Ok(None)
        }
    }

    def set(&self, pos: u64) -> PyResult<PyNone> {
        let state = self.state(py).borrow();
        if let Some(bar) = state.running() {
            bar.set(pos).map_pyerr(py)?;
        }
        Ok(PyNone)
    }

    def set_total(&self, total: Option<u64>) -> PyResult<PyNone> {
        let state = self.state(py).borrow();
        if let Some(bar) = state.running() {
            bar.set_total(total).map_pyerr(py)?;
        }
        Ok(PyNone)
    }

    def increment(&self, delta: u64) -> PyResult<PyNone> {
        let state = self.state(py).borrow();
        if let Some(bar) = state.running() {
            bar.increment(delta).map_pyerr(py)?;
        }
        Ok(PyNone)
    }

    def set_message(&self, message: &str) -> PyResult<PyNone> {
        let state = self.state(py).borrow();
        if let Some(bar) = state.running() {
            bar.set_message(message).map_pyerr(py)?;
        }
        Ok(PyNone)
    }

});
