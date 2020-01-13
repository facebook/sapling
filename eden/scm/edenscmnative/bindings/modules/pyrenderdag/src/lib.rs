/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::sync::Arc;

use cpython::*;
use parking_lot::Mutex;
use renderdag::{Ancestor, GraphRowRenderer, Renderer};

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "renderdag"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "ascii", py_fn!(py, ascii(min_height: usize)))?;
    m.add(py, "asciilarge", py_fn!(py, asciilarge(min_height: usize)))?;
    m.add(py, "lines", py_fn!(py, lines(min_height: usize)))?;
    m.add(
        py,
        "linessquare",
        py_fn!(py, linessquare(min_height: usize)),
    )?;
    m.add(py, "linesdec", py_fn!(py, linesdec(min_height: usize)))?;
    Ok(m)
}

fn convert_parents(py: Python, parents: Vec<(String, i64)>) -> PyResult<Vec<Ancestor<i64>>> {
    parents
        .into_iter()
        .map(|(kind, parent)| match kind.as_str() {
            "P" => Ok(Ancestor::Parent(parent)),
            "G" => Ok(Ancestor::Ancestor(parent)),
            "M" => Ok(Ancestor::Anonymous),
            _ => Err(PyErr::new::<exc::ValueError, _>(
                py,
                format!("unknown parent type: {}", kind),
            )),
        })
        .collect()
}

py_class!(pub class renderer |py| {
    data inner: Arc<Mutex<dyn Renderer<i64, Output = String> + Send>>;

    def width(&self, node: Option<i64>, parents: Option<Vec<(String, i64)>>) -> PyResult<u64> {
        let renderer = self.inner(py).lock();
        let parents = parents.map(|parents| convert_parents(py, parents)).transpose()?;
        Ok(renderer.width(node.as_ref(), parents.as_ref()))
    }

    def reserve(&self, node: i64) -> PyResult<PyObject> {
        let mut renderer = self.inner(py).lock();
        renderer.reserve(node);
        Ok(py.None())
    }

    def nextrow(&self, node: i64, parents: Vec<(String, i64)>, glyph: String, message: String) -> PyResult<String> {
        let mut renderer = self.inner(py).lock();
        Ok(renderer.next_row(node, convert_parents(py, parents)?, glyph, message))
    }
});

fn ascii(py: Python, min_height: usize) -> PyResult<renderer> {
    let renderer = Arc::new(Mutex::new(
        GraphRowRenderer::new()
            .output()
            .with_min_row_height(min_height)
            .build_ascii(),
    ));
    renderer::create_instance(py, renderer)
}

fn asciilarge(py: Python, min_height: usize) -> PyResult<renderer> {
    let renderer = Arc::new(Mutex::new(
        GraphRowRenderer::new()
            .output()
            .with_min_row_height(min_height)
            .build_ascii_large(),
    ));
    renderer::create_instance(py, renderer)
}

fn lines(py: Python, min_height: usize) -> PyResult<renderer> {
    let renderer = Arc::new(Mutex::new(
        GraphRowRenderer::new()
            .output()
            .with_min_row_height(min_height)
            .build_box_drawing(),
    ));
    renderer::create_instance(py, renderer)
}

fn linessquare(py: Python, min_height: usize) -> PyResult<renderer> {
    let renderer = Arc::new(Mutex::new(
        GraphRowRenderer::new()
            .output()
            .with_min_row_height(min_height)
            .build_box_drawing()
            .with_square_glyphs(),
    ));
    renderer::create_instance(py, renderer)
}

fn linesdec(py: Python, min_height: usize) -> PyResult<renderer> {
    let renderer = Arc::new(Mutex::new(
        GraphRowRenderer::new()
            .output()
            .with_min_row_height(min_height)
            .build_box_drawing()
            .with_dec_graphics_glyphs(),
    ));
    renderer::create_instance(py, renderer)
}
