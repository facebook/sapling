/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_runtime::try_block_unless_interrupted as block_on;
use cpython::*;
use cpython_ext::ResultPyErrExt;
use dag::ops::Parents as DagParents;
use dag::Vertex;

// A wrapper around [`DagParents`].
py_class!(pub class parents |py| {
    data inner: Box<dyn DagParents>;

    def __call__(&self, vertex: PyBytes) -> PyResult<Vec<PyBytes>> {
        let vertex = Vertex::copy_from(vertex.data(py));
        let parents: Vec<Vertex> = block_on(self.inner(py).parent_names(vertex)).map_pyerr(py)?;
        let parents: Vec<PyBytes> = parents.into_iter().map(|v| PyBytes::new(py, v.as_ref())).collect();
        Ok(parents)
    }
});
