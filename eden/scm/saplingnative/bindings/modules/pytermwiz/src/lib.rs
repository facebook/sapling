/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_local_definitions)]

use std::sync::RwLock;
use std::time::Duration;

use ::serde::Deserialize;
use ::serde::Serialize;
use cpython::*;
use cpython_ext::ResultPyErrExt;
use cpython_ext::convert::Serde;
use termwiz::caps::Capabilities;
use termwiz::caps::ColorLevel;
use termwiz::caps::ProbeHints;
use termwiz::input::InputEvent;
use termwiz::input::KeyEvent;
use termwiz::input::MouseEvent;
use termwiz::input::PixelMouseEvent;
use termwiz::surface::Change;
use termwiz::surface::CursorVisibility;
use termwiz::surface::SequenceNo;
use termwiz::terminal::SystemTerminal;
use termwiz::terminal::Terminal;
use termwiz::terminal::buffered::BufferedTerminal as NativeBufferedTerminal;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "termwiz"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<BufferedTerminal>(py)?;
    Ok(m)
}

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

    def dimensions(&self) -> PyResult<(usize, usize)> {
        let inner = self.inner(py).read().unwrap();
        Ok(inner.dimensions())
    }

    def cursor_position(&self) -> PyResult<(usize, usize)> {
        let inner = self.inner(py).read().unwrap();
        Ok(inner.cursor_position())
    }

    def cursor_visibility(&self) -> PyResult<bool> {
        let inner = self.inner(py).read().unwrap();
        let result = inner.cursor_visibility();
        let result = match result {
            CursorVisibility::Hidden => false,
            CursorVisibility::Visible => true,
        };
        Ok(result)
    }

    def title(&self) -> PyResult<String> {
        let inner = self.inner(py).read().unwrap();
        let result = inner.title();
        Ok(result.to_owned())
    }

    def resize(&self, width: usize, height: usize) -> PyResult<PyNone> {
        let mut inner = self.inner(py).write().unwrap();
        inner.resize(width, height);
        Ok(PyNone)
    }

    def add_changes(&self, changes: Serde<Vec<Change>>) -> PyResult<SequenceNo> {
        let mut inner = self.inner(py).write().unwrap();
        let result = inner.add_changes(changes.0);
        Ok(result)
    }

    def add_change(&self, change: Serde<Change>) -> PyResult<SequenceNo> {
        let mut inner = self.inner(py).write().unwrap();
        let result = inner.add_change(change.0);
        Ok(result)
    }

    def screen_chars_to_string(&self) -> PyResult<String> {
        let inner = self.inner(py).read().unwrap();
        let result = inner.screen_chars_to_string();
        Ok(result)
    }

    def has_changes(&self, seq: SequenceNo) -> PyResult<bool> {
        let inner = self.inner(py).read().unwrap();
        let result = inner.has_changes(seq);
        Ok(result)
    }

    def current_seqno(&self) -> PyResult<SequenceNo> {
        let inner = self.inner(py).read().unwrap();
        let result = inner.current_seqno();
        Ok(result)
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

// Workaround of InputEvent didn't implement serde.
// PR: https://github.com/wezterm/wezterm/pull/7266
#[derive(Serialize, Deserialize)]
#[serde(rename = "InputEvent")]
enum InputEventSerde {
    Key(KeyEvent),
    Mouse(MouseEvent),
    PixelMouse(PixelMouseEvent),
    Resized { cols: usize, rows: usize },
    Paste(String),
    Wake,
}

impl From<InputEvent> for InputEventSerde {
    fn from(value: InputEvent) -> Self {
        match value {
            InputEvent::Key(v) => Self::Key(v),
            InputEvent::Mouse(v) => Self::Mouse(v),
            InputEvent::PixelMouse(v) => Self::PixelMouse(v),
            InputEvent::Resized { cols, rows } => Self::Resized { cols, rows },
            InputEvent::Paste(v) => Self::Paste(v),
            InputEvent::Wake => Self::Wake,
        }
    }
}

trait MapTermwizError<T> {
    fn pyerr(self, py: Python) -> PyResult<T>;
}

impl<T> MapTermwizError<T> for termwiz::Result<T> {
    fn pyerr(self, py: Python) -> PyResult<T> {
        self.map_err(anyhow::Error::from).map_pyerr(py)
    }
}
