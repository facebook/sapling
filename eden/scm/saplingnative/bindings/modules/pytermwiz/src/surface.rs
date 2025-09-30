/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::RwLock;

use cpython::*;
use termwiz::surface::CursorVisibility;
use termwiz::surface::SequenceNo;
use termwiz::surface::Surface as NativeSurface;

use crate::Change;

py_class!(pub class Surface |py| {
    data inner: Box<dyn WithSurface>;

    def __new__(_cls, width: usize, height: usize) -> PyResult<Self> {
        let surface = NativeSurface::new(width, height);
        let surface = RwLock::new(surface);
        Self::create_instance(py, Box::new( surface))
    }

    def dimensions(&self) -> PyResult<(usize, usize)> {
        let mut result = (0, 0);
        self.inner(py).with_surface(py, &mut |surface| {
            result = surface.dimensions();
        });
        Ok(result)
    }

    def cursor_position(&self) -> PyResult<(usize, usize)> {
        let mut result = (0, 0);
        self.inner(py).with_surface(py, &mut |surface| {
            result = surface.cursor_position();
        });
        Ok(result)
    }

    def cursor_visibility(&self) -> PyResult<bool> {
        let mut result = false;
        self.inner(py).with_surface(py, &mut |surface| {
            result = match surface.cursor_visibility() {
                CursorVisibility::Hidden => false,
                CursorVisibility::Visible => true,
            };
        });
        Ok(result)
    }

    def title(&self) -> PyResult<String> {
        let mut result = String::new();
        self.inner(py).with_surface(py, &mut |surface| {
            result = surface.title().to_owned();
        });
        Ok(result)
    }

    def resize(&self, width: usize, height: usize) -> PyResult<PyNone> {
        self.inner(py).with_surface_mut(py, &mut |surface| {
            surface.resize(width, height);
        });
        Ok(PyNone)
    }

    def add_changes(&self, changes: Vec<Change>) -> PyResult<SequenceNo> {
        let mut result = 0;
        self.inner(py).with_surface_mut(py, &mut |surface| {
            let changes = changes.iter().map(|v| v.to_native(py)).collect();
            result = surface.add_changes(changes).to_owned();
        });
        Ok(result)
    }

    def add_change(&self, change: Change) -> PyResult<SequenceNo> {
        let mut result = 0;
        self.inner(py).with_surface_mut(py, &mut |surface| {
            let change = change.to_native(py);
            result = surface.add_change(change).to_owned();
        });
        Ok(result)
    }

    def screen_chars_to_string(&self) -> PyResult<String> {
        let mut result = String::new();
        self.inner(py).with_surface(py, &mut |surface| {
            result = surface.screen_chars_to_string().to_owned();
        });
        Ok(result)
    }

    def has_changes(&self, seq: SequenceNo) -> PyResult<bool> {
        let mut result = false;
        self.inner(py).with_surface(py, &mut |surface| {
            result = surface.has_changes(seq);
        });
        Ok(result)
    }

    def current_seqno(&self) -> PyResult<SequenceNo> {
        let mut result = 0;
        self.inner(py).with_surface(py, &mut |surface| {
            result = surface.current_seqno();
        });
        Ok(result)
    }

    def diff_screens(&self, other: Surface) -> PyResult<Vec<Change>> {
        let mut result: PyResult<Vec::<Change>> = Ok(Vec::new());
        self.inner(py).with_surface(py, &mut |self_surface| {
            other.inner(py).with_surface(py, &mut |other_surface| {
                let native_changes = self_surface.diff_screens(other_surface);
                result = native_changes.into_iter().map(|c| Change::create_instance(py, c)).collect();
            });
        });
        result
    }

    def diff_region(&self, x: usize, y: usize, width: usize, height: usize, other: Surface, other_x: usize, other_y: usize) -> PyResult<Vec<Change>> {
        let mut result: PyResult<Vec::<Change>> = Ok(Vec::new());
        self.inner(py).with_surface(py, &mut |self_surface| {
            other.inner(py).with_surface(py, &mut |other_surface| {
                let native_changes = self_surface.diff_region(x, y, width, height, other_surface, other_x, other_y);
                result = native_changes.into_iter().map(|c| Change::create_instance(py, c)).collect();
            });
        });
        result
    }

    /// Draw the contents of other into self at the specified coordinates.
    /// Size is decided by the other surface. Calls `diff_region` under the hood.
    def draw_from_screen(&self, other: Surface, x: usize = 0, y: usize = 0) -> PyResult<SequenceNo> {
        // sanity check to avoid deadlock.
        let mut same_surface = false;
        self.inner(py).with_surface(py, &mut |self_surface| {
            other.inner(py).with_surface(py, &mut |other_surface| {
                same_surface = std::ptr::eq(self_surface, other_surface);
            });
        });
        if same_surface {
            return Err(PyErr::new::<exc::ValueError, _>(py, "cannot draw to the same surface"));
        }

        let mut result = 0;
        self.inner(py).with_surface_mut(py, &mut |self_surface| {
            other.inner(py).with_surface(py, &mut |other_surface| {
                result = self_surface.draw_from_screen(other_surface, x, y);
            });
        });
        Ok(result)
    }
});

/// Abstraction to get a `Surface` reference.
/// This is to support both `RwLock<BufferedTerminal>` and `RwLock<Surface>`.
pub trait WithSurface: Send + Sync {
    fn with_surface(&self, py: Python, f: &mut dyn for<'a> FnMut(&'a NativeSurface));
    fn with_surface_mut(&self, py: Python, f: &mut dyn for<'a> FnMut(&'a mut NativeSurface));
}

impl WithSurface for RwLock<NativeSurface> {
    fn with_surface(&self, _py: Python, f: &mut dyn for<'a> FnMut(&'a NativeSurface)) {
        let surface = self.read().unwrap();
        f(&surface)
    }

    fn with_surface_mut(&self, _py: Python, f: &mut dyn for<'a> FnMut(&'a mut NativeSurface)) {
        let mut surface = self.write().unwrap();
        f(&mut surface)
    }
}
