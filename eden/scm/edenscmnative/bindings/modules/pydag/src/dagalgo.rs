/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::VecDeque;
use std::sync::Arc;

use async_runtime::try_block_unless_interrupted as block_on;
use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::convert::Serde;
use cpython_ext::ResultPyErrExt;
use cpython_ext::Str;
use dag::DagAlgorithm;
use dag::IdSegment;
use dag::Set;
use dag::Vertex;

use crate::Names;

py_class!(pub class dagalgo |py| {
    data dag: Arc<dyn DagAlgorithm + Send + Sync + 'static>;

    /// Sort a set.
    def sort(&self, set: Names) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).sort(&set.0)).map_pyerr(py)?))
    }

    def all(&self) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).all()).map_pyerr(py)?))
    }

    /// Return a set including vertexes in the master group.
    def mastergroup(&self) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).master_group()).map_pyerr(py)?))
    }

    /// Calculate all ancestors reachable from the set.
    def ancestors(&self, set: Names) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).ancestors(set.0)).map_pyerr(py)?))
    }

    /// Calculate all ancestors reachable from the set, following only first parents.
    def firstancestors(&self, set: Names) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).first_ancestors(set.0)).map_pyerr(py)?))
    }

    /// Calculate parents of the given set.
    def parents(&self, set: Names) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).parents(set.0)).map_pyerr(py)?))
    }

    /// Get parents of a single `name`. Preserve the order.
    def parentnames(&self, name: PyBytes) -> PyResult<Vec<PyBytes>> {
        let parents = block_on(self.dag(py).parent_names(Vertex::copy_from(name.data(py)))).map_pyerr(py)?;
        Ok(parents.into_iter().map(|name| PyBytes::new(py, name.as_ref())).collect())
    }

    /// The `n`-th first ancestor of `x`
    def firstancestornth(&self, x: PyBytes, n: u64) -> PyResult<Option<PyBytes>> {
        let result = block_on(self.dag(py).first_ancestor_nth(Vertex::copy_from(x.data(py)), n)).map_pyerr(py)?;
        Ok(result.map(|v| PyBytes::new(py, v.as_ref())))
    }

    /// Calculate parents of the given set.
    def heads(&self, set: Names) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).heads(set.0)).map_pyerr(py)?))
    }

    /// Calculate children of the given set.
    def children(&self, set: Names) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).children(set.0)).map_pyerr(py)?))
    }

    /// Calculate roots of the given set.
    def roots(&self, set: Names) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).roots(set.0)).map_pyerr(py)?))
    }

    /// Calculate merges of the given set.
    def merges(&self, set: Names) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).merges(set.0)).map_pyerr(py)?))
    }

    /// Calculate one greatest common ancestor of a set.
    /// If there are multiple greatest common ancestors, pick an arbitrary one.
    def gcaone(&self, set: Names) -> PyResult<Option<PyBytes>> {
        Ok(block_on(self.dag(py).gca_one(set.0)).map_pyerr(py)?.map(|name| PyBytes::new(py, name.as_ref())))
    }

    /// Calculate all greatest common ancestors of a set.
    def gcaall(&self, set: Names) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).gca_all(set.0)).map_pyerr(py)?))
    }

    /// Calculate all common ancestors of a set.
    def commonancestors(&self, set: Names) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).common_ancestors(set.0)).map_pyerr(py)?))
    }

    /// Check if `ancestor` is an ancestor of `descendant`.
    def isancestor(&self, ancestor: PyBytes, descendant: PyBytes) -> PyResult<bool> {
        let ancestor = Vertex::copy_from(ancestor.data(py));
        let descendant = Vertex::copy_from(descendant.data(py));
        block_on(self.dag(py).is_ancestor(ancestor, descendant)).map_pyerr(py)
    }

    /// Calculate `heads(ancestors(set))`.
    /// This is faster than calling `heads` and `ancestors` individually.
    def headsancestors(&self, set: Names) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).heads_ancestors(set.0)).map_pyerr(py)?))
    }

    /// Calculate `roots::heads`.
    def range(&self, roots: Names, heads: Names) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).range(roots.0, heads.0)).map_pyerr(py)?))
    }

    /// Calculate `reachable % unreachable`.
    def only(&self, reachable: Names, unreachable: Names) -> PyResult<Names> {
        let result = block_on(self.dag(py).only(reachable.0, unreachable.0)).map_pyerr(py)?;
        Ok(Names(result))
    }

    /// Calculate `reachable % unreachable`, and `::unreachable`.
    def onlyboth(&self, reachable: Names, unreachable: Names) -> PyResult<(Names, Names)> {
        let (reachable_ancestors, unreachable_ancestors) =
            block_on(self.dag(py).only_both(reachable.0, unreachable.0)).map_pyerr(py)?;
        Ok((Names(reachable_ancestors), Names(unreachable_ancestors)))
    }

    /// Calculate descendants of the given set.
    def descendants(&self, set: Names) -> PyResult<Names> {
        Ok(Names(block_on(self.dag(py).descendants(set.0)).map_pyerr(py)?))
    }

    /// Calculate `roots & (heads | parents(only(heads, roots & ancestors(heads))))`.
    def reachableroots(&self, roots: Names, heads: Names) -> PyResult<Names> {
        let result = block_on(self.dag(py).reachable_roots(roots.0, heads.0)).map_pyerr(py)?;
        Ok(Names(result))
    }

    /// Return true if the vertexes are lazily fetched from remote.
    def isvertexlazy(&self) -> PyResult<bool> {
        Ok(self.dag(py).is_vertex_lazy())
    }

    /// Beautify the graph so `render` might look better.
    def beautify(&self, mainbranch: Option<Names> = None) -> PyResult<Self> {
        let dag = block_on(self.dag(py).beautify(mainbranch.map(|h| h.0))).map_pyerr(py)?;
        Self::from_dag(py, dag)
    }

    /// Extract a subdag from the graph.
    def subdag(&self, set: Names) -> PyResult<Self> {
        let dag = block_on(self.dag(py).subdag(set.0)).map_pyerr(py)?;
        Self::from_dag(py, dag)
    }

    /// Render the graph into an ASCII string.
    def render(&self, getmessage: Option<PyObject> = None) -> PyResult<Str> {
        let get_message = move |vertex: &Vertex| -> Option<String> {
            if let Some(getmessage) = &getmessage {
                if getmessage.is_callable(py) {
                    if let Ok(message) = getmessage.call(py, (PyBytes::new(py, vertex.as_ref()),), None) {
                        if let Ok(message) = message.extract::<String>(py) {
                            return Some(message)
                        }
                    }
                }
            }
            None
        };
        let dag = self.dag(py);
        Ok(renderdag::render_namedag(dag.as_ref(), get_message).map_pyerr(py)?.into())
    }

    /// segments(nameset, maxlevel=255) -> [segment]
    /// Get the segments covering the set with specified maximum level.
    def segments(&self, set: ImplInto<Set>, maxlevel: u8 = 255) -> PyResult<Serde<VecDeque<IdSegment>>> {
        let set = set.into();
        let (id_set, _id_map) = match set.to_id_set_and_id_map_in_o1() {
            Some(v) => v,
            None => {
                let msg = format!("{:?} cannot be converted to IdSet", &set);
                return Err(PyErr::new::<exc::ValueError, _>(py, msg));
            }
        };
        let id_dag = self.dag(py).id_dag_snapshot().map_pyerr(py)?;
        let segments = id_dag.id_set_to_id_segments_with_max_level(&id_set, maxlevel).map_pyerr(py)?;
        Ok(Serde(segments))
    }
});

impl dagalgo {
    pub fn from_dag(py: Python, dag: impl DagAlgorithm + Send + Sync + 'static) -> PyResult<Self> {
        Self::create_instance(py, Arc::new(dag))
    }

    pub fn from_arc_dag(py: Python, dag: Arc<dyn DagAlgorithm + Send + Sync>) -> PyResult<Self> {
        Self::create_instance(py, dag)
    }
}
