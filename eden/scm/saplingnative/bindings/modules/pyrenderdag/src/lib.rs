/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::sync::Arc;

use cpython::*;
use cpython_ext::PyNone;
use minibytes::Bytes;
use parking_lot::Mutex;
use renderdag::Ancestor;
use renderdag::GraphRowRenderer;
use renderdag::Renderer;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "renderdag"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "ascii", py_fn!(py, ascii(min_height: usize)))?;
    m.add(py, "asciilarge", py_fn!(py, asciilarge(min_height: usize)))?;
    m.add(
        py,
        "linescurved",
        py_fn!(py, linescurved(min_height: usize)),
    )?;
    m.add(
        py,
        "linessquare",
        py_fn!(py, linessquare(min_height: usize)),
    )?;
    m.add(py, "linesdec", py_fn!(py, linesdec(min_height: usize)))?;
    m.add(py, "linescurvedchars", "─│╷╯╰┴╮╭┬┤├┼~")?;
    m.add(py, "linessquarechars", "─│·┘└┴┐┌┬┤├┼~")?;
    Ok(m)
}

pub struct PyNode(Bytes);

impl From<PyNode> for Bytes {
    fn from(pynode: PyNode) -> Bytes {
        pynode.0
    }
}

impl<'a> FromPyObject<'a> for PyNode {
    fn extract(py: Python, obj: &'a PyObject) -> PyResult<Self> {
        if let Ok(node) = obj.extract::<PyBytes>(py) {
            Ok(PyNode(Bytes::copy_from_slice(node.data(py))))
        } else if let Ok(rev) = obj.extract::<i64>(py) {
            let slice: [u8; 8] = unsafe { std::mem::transmute(rev.to_be()) };
            Ok(PyNode(Bytes::copy_from_slice(&slice)))
        } else {
            Err(PyErr::new::<exc::TypeError, _>(py, "expect bytes or int"))
        }
    }
}

fn convert_parents(py: Python, parents: Vec<(String, PyNode)>) -> PyResult<Vec<Ancestor<Bytes>>> {
    parents
        .into_iter()
        .map(|(kind, parent)| match kind.as_str() {
            "P" => Ok(Ancestor::Parent(parent.into())),
            "G" => Ok(Ancestor::Ancestor(parent.into())),
            "M" => Ok(Ancestor::Anonymous),
            _ => Err(PyErr::new::<exc::ValueError, _>(
                py,
                format!("unknown parent type: {}", kind),
            )),
        })
        .collect()
}

py_class!(pub class renderer |py| {
    data inner: Arc<Mutex<dyn Renderer<Bytes, Output = String> + Send>>;

    def width(&self, node: Option<PyNode>, parents: Option<Vec<(String, PyNode)>>) -> PyResult<u64> {
        let renderer = self.inner(py).lock();
        let parents = parents.map(|parents| convert_parents(py, parents)).transpose()?;
        let node: Option<Bytes> = node.map(Into::into);
        Ok(renderer.width(node.as_ref(), parents.as_ref()))
    }

    def reserve(&self, node: PyNode) -> PyResult<PyNone> {
        let mut renderer = self.inner(py).lock();
        renderer.reserve(node.into());
        Ok(PyNone)
    }

    def nextrow(&self, node: PyNode, parents: Vec<(String, PyNode)>, glyph: String, message: String) -> PyResult<String> {
        let mut renderer = self.inner(py).lock();
        Ok(renderer.next_row(node.into(), convert_parents(py, parents)?, glyph, message))
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

fn linescurved(py: Python, min_height: usize) -> PyResult<renderer> {
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
