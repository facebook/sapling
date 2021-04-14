/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_runtime::block_on_exclusive as block_on;
use cpython::*;
use cpython_ext::ResultPyErrExt;
use dag::ops::IdConvert;
use dag::Id;
use dag::Vertex;
use std::sync::Arc;

// Mercurial's special case. -1 maps to (b"\0" * 20)
pub(crate) const NULL_NODE: [u8; 20] = [0u8; 20];

py_class!(pub class idmap |py| {
    data map: Arc<dyn IdConvert + Send + Sync + 'static>;

    /// Translate id to node.
    def id2node(&self, id: i64) -> PyResult<PyBytes> {
        if id == -1 {
            Ok(PyBytes::new(py, &NULL_NODE))
        } else {
            let v = block_on(self.map(py).vertex_name(Id(id as u64))).map_pyerr(py)?;
            Ok(PyBytes::new(py, v.as_ref()))
        }
    }

    /// Translate node to id.
    def node2id(&self, node: PyBytes) -> PyResult<i64> {
        let node = node.data(py);
        if node == &NULL_NODE {
            Ok(-1)
        } else {
            let id = block_on(self.map(py).vertex_id(Vertex::copy_from(node))).map_pyerr(py)?;
            Ok(id.0 as i64)
        }
    }

    /// Filter out nodes not in the IdMap.
    /// (nodes, inverse=False) -> nodes.
    ///
    /// Use batching internally. Faster than checking `__contains__`
    /// one by one.
    ///
    /// If inverse is set to True, return missing nodes instead of
    /// present nodes.
    def filternodes(&self, nodes: Vec<PyBytes>, inverse: bool = false) -> PyResult<Vec<PyBytes>> {
        let map = self.map(py);
        let vertexes: Vec<_> = nodes.iter().map(|n| Vertex::copy_from(n.data(py))).collect();
        let ids = block_on(map.vertex_id_batch(&vertexes)).map_pyerr(py)?;
        let mut result = Vec::with_capacity(nodes.len());
        for (node, id) in nodes.into_iter().zip(ids) {
            let present = match id {
                Err(dag::Error::VertexNotFound(_)) => false,
                Ok(_) => true,
                Err(e) => return Err(e).map_pyerr(py),
            };
            match (present, inverse) {
                (true, false) | (false, true) => result.push(node),
                _ => {}
            }
        }
        Ok(result)
    }

    /// Lookup nodes by hex prefix.
    def hexprefixmatch(&self, prefix: PyObject, limit: usize = 5) -> PyResult<Vec<PyBytes>> {
        let prefix: Vec<u8> = if let Ok(bytes) = prefix.extract::<PyBytes>(py) {
            bytes.data(py).to_vec()
        } else {
            prefix.extract::<String>(py)?.as_bytes().to_vec()
        };
        if !prefix.iter().all(|&b| (b >= b'0' && b <= b'9') || (b >= b'a' && b <= b'f')) {
            // Invalid hex prefix. Pretend nothing matches.
            return Ok(Vec::new())
        }
        let map = self.map(py).clone();
        let vertexes = async_runtime::block_on_future(async move {
            map.vertexes_by_hex_prefix(&prefix, limit).await
        });
        let nodes = vertexes
            .map_pyerr(py)?
            .into_iter()
            .map(|s| PyBytes::new(py, s.as_ref()))
            .collect();
        Ok(nodes)
    }

    def __contains__(&self, node: PyBytes) -> PyResult<bool> {
        let node = node.data(py);
        if node == &NULL_NODE {
            Ok(true)
        } else {
            let name = Vertex::copy_from(node);
            Ok(block_on(self.map(py).contains_vertex_name(&name)).map_pyerr(py)?)
        }
    }
});

impl idmap {
    pub(crate) fn from_arc_idmap(
        py: Python,
        map: Arc<dyn IdConvert + Send + Sync>,
    ) -> PyResult<Self> {
        Self::create_instance(py, map)
    }
}
