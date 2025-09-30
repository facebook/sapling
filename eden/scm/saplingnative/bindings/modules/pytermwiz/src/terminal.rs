/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::RwLock;
use std::time::Duration;

use cpython::*;
use cpython_ext::convert::Serde;
use termwiz::caps::Capabilities;
use termwiz::caps::ColorLevel;
use termwiz::caps::ProbeHints;
use termwiz::surface::Surface as NativeSurface;
use termwiz::terminal::SystemTerminal;
use termwiz::terminal::Terminal;
use termwiz::terminal::buffered::BufferedTerminal as NativeBufferedTerminal;

use crate::InputEventSerde;
use crate::MapTermwizError;
use crate::Surface;
use crate::surface::WithSurface;

py_class!(pub class BufferedTerminal |py| {
    data inner: RwLock<NativeBufferedTerminal<SystemTerminal>>;

    def __new__(_cls) -> PyResult<Self> {
        let hints = ProbeHints::new_from_env()
            .color_level(Some(ColorLevel::TrueColor))
            .mouse_reporting(Some(false));
        let caps = Capabilities::new_with_hints(hints).pyerr(py)?;
        let system_terminal = SystemTerminal::new(caps.clone()).or_else(|_| SystemTerminal::new_from_stdio(caps)).pyerr(py)?;
        let terminal = NativeBufferedTerminal::new(system_terminal).pyerr(py)?;
        Self::create_instance(py, RwLock::new(terminal))
    }

    def flush(&self) -> PyResult<PyNone> {
        let mut inner = self.inner(py).write().unwrap();
        inner.flush().pyerr(py)?;
        Ok(PyNone)
    }

    def repaint(&self) -> PyResult<PyNone> {
        let mut inner = self.inner(py).write().unwrap();
        inner.repaint().pyerr(py)?;
        Ok(PyNone)
    }

    def check_for_resize(&self) -> PyResult<bool> {
        let mut inner = self.inner(py).write().unwrap();
        let result = inner.check_for_resize().pyerr(py)?;
        Ok(result)
    }

    // Deref<Target=Surface>

    @property
    def surface(&self) -> PyResult<Surface> {
        let inner = self.clone_ref(py);
        Surface::create_instance(py, Box::new(inner))
    }

    // trait Terminal

    def set_raw_mode(&self) -> PyResult<PyNone> {
        let mut inner = self.inner(py).write().unwrap();
        inner.terminal().set_raw_mode().pyerr(py)?;
        Ok(PyNone)
    }

    def set_cooked_mode(&self) -> PyResult<PyNone> {
        let mut inner = self.inner(py).write().unwrap();
        inner.terminal().set_cooked_mode().pyerr(py)?;
        Ok(PyNone)
    }

    def enter_alternate_screen(&self) -> PyResult<PyNone> {
        let mut inner = self.inner(py).write().unwrap();
        inner.terminal().enter_alternate_screen().pyerr(py)?;
        Ok(PyNone)
    }

    def exit_alternate_screen(&self) -> PyResult<PyNone> {
        let mut inner = self.inner(py).write().unwrap();
        inner.terminal().exit_alternate_screen().pyerr(py)?;
        Ok(PyNone)
    }

    def poll_input(&self, wait_ms: Option<u64> = None) -> PyResult<Option<Serde<InputEventSerde>>> {
        let mut inner = self.inner(py).write().unwrap();
        let wait = wait_ms.map( Duration::from_millis);
        let result = inner.terminal().poll_input(wait).pyerr(py)?;
        let result = result.map(|v| Serde(InputEventSerde::from(v)));
        Ok(result)
    }
});

impl WithSurface for BufferedTerminal {
    fn with_surface(&self, py: Python, f: &mut dyn for<'a> FnMut(&'a NativeSurface)) {
        let inner = self.inner(py);
        let inner = inner.read().unwrap();
        f(&inner)
    }

    fn with_surface_mut(&self, py: Python, f: &mut dyn for<'a> FnMut(&'a mut NativeSurface)) {
        let inner = self.inner(py);
        let mut inner = inner.write().unwrap();
        f(&mut inner)
    }
}
