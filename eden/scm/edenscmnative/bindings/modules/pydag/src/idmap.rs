/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::ResultPyErrExt;
use dag::ops::IdConvert;
use dag::ops::PrefixLookup;
use dag::Id;
use dag::Vertex;

/// A combination of IdConvert + PrefixLookup.
pub trait IdMap: IdConvert + PrefixLookup {}
impl<T> IdMap for T where T: IdConvert + PrefixLookup {}

// Mercurial's special case. -1 maps to (b"\0" * 20)
pub(crate) const NULL_NODE: [u8; 20] = [0u8; 20];

py_class!(pub class idmap |py| {
    data map: Box<dyn IdMap + Send + 'static>;

    /// Translate id to node.
    def id2node(&self, id: i64) -> PyResult<PyBytes> {
        if id == -1 {
            Ok(PyBytes::new(py, &NULL_NODE))
        } else {
            let v = self.map(py).vertex_name(Id(id as u64)).map_pyerr(py)?;
            Ok(PyBytes::new(py, v.as_ref()))
        }
    }

    /// Translate node to id.
    def node2id(&self, node: PyBytes) -> PyResult<i64> {
        let node = node.data(py);
        if node == &NULL_NODE {
            Ok(-1)
        } else {
            let id = self.map(py).vertex_id(Vertex::copy_from(node)).map_pyerr(py)?;
            Ok(id.0 as i64)
        }
    }

    /// Lookup nodes by hex prefix.
    def hexprefixmatch(&self, prefix: PyBytes, limit: usize = 5) -> PyResult<Vec<PyBytes>> {
        let prefix = prefix.data(py);
        if !prefix.iter().all(|&b| (b >= b'0' && b <= b'9') || (b >= b'a' && b <= b'f')) {
            // Invalid hex prefix. Pretend nothing matches.
            return Ok(Vec::new())
        }
        let nodes = self.map(py)
            .vertexes_by_hex_prefix(prefix, limit)
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
            Ok(self.map(py).contains_vertex_name(&name).map_pyerr(py)?)
        }
    }
});

impl idmap {
    pub(crate) fn from_idmap(py: Python, map: impl IdMap + Send + 'static) -> PyResult<Self> {
        Self::create_instance(py, Box::new(map))
    }
}
