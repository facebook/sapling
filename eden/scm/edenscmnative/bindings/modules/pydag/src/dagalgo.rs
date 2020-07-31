/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::Names;
use cpython::*;
use cpython_ext::ResultPyErrExt;
use cpython_ext::Str;
use dag::DagAlgorithm;
use dag::InverseDag;
use dag::Vertex;
use std::sync::Arc;

py_class!(pub class dagalgo |py| {
    // Arc is used for 'inverse'.
    data dag: Arc<dyn DagAlgorithm + Send + Sync + 'static>;

    /// Sort a set.
    def sort(&self, set: Names) -> PyResult<Names> {
        Ok(Names(self.dag(py).sort(&set.0).map_pyerr(py)?))
    }

    def all(&self) -> PyResult<Names> {
        Ok(Names(self.dag(py).all().map_pyerr(py)?))
    }

    /// Calculate all ancestors reachable from the set.
    def ancestors(&self, set: Names) -> PyResult<Names> {
        Ok(Names(self.dag(py).ancestors(set.0).map_pyerr(py)?))
    }

    /// Calculate parents of the given set.
    def parents(&self, set: Names) -> PyResult<Names> {
        Ok(Names(self.dag(py).parents(set.0).map_pyerr(py)?))
    }

    /// Get parents of a single `name`. Preserve the order.
    def parentnames(&self, name: PyBytes) -> PyResult<Vec<PyBytes>> {
        let parents = self.dag(py).parent_names(Vertex::copy_from(name.data(py))).map_pyerr(py)?;
        Ok(parents.into_iter().map(|name| PyBytes::new(py, name.as_ref())).collect())
    }

    /// Calculate parents of the given set.
    def heads(&self, set: Names) -> PyResult<Names> {
        Ok(Names(self.dag(py).heads(set.0).map_pyerr(py)?))
    }

    /// Calculate children of the given set.
    def children(&self, set: Names) -> PyResult<Names> {
        Ok(Names(self.dag(py).children(set.0).map_pyerr(py)?))
    }

    /// Calculate roots of the given set.
    def roots(&self, set: Names) -> PyResult<Names> {
        Ok(Names(self.dag(py).roots(set.0).map_pyerr(py)?))
    }

    /// Calculate one greatest common ancestor of a set.
    /// If there are multiple greatest common ancestors, pick an arbitrary one.
    def gcaone(&self, set: Names) -> PyResult<Option<PyBytes>> {
        Ok(self.dag(py).gca_one(set.0).map_pyerr(py)?.map(|name| PyBytes::new(py, name.as_ref())))
    }

    /// Calculate all greatest common ancestors of a set.
    def gcaall(&self, set: Names) -> PyResult<Names> {
        Ok(Names(self.dag(py).gca_all(set.0).map_pyerr(py)?))
    }

    /// Calculate all common ancestors of a set.
    def commonancestors(&self, set: Names) -> PyResult<Names> {
        Ok(Names(self.dag(py).common_ancestors(set.0).map_pyerr(py)?))
    }

    /// Check if `ancestor` is an ancestor of `descendant`.
    def isancestor(&self, ancestor: PyBytes, descendant: PyBytes) -> PyResult<bool> {
        let ancestor = Vertex::copy_from(ancestor.data(py));
        let descendant = Vertex::copy_from(descendant.data(py));
        self.dag(py).is_ancestor(ancestor, descendant).map_pyerr(py)
    }

    /// Calculate `heads(ancestors(set))`.
    /// This is faster than calling `heads` and `ancestors` individually.
    def headsancestors(&self, set: Names) -> PyResult<Names> {
        Ok(Names(self.dag(py).heads_ancestors(set.0).map_pyerr(py)?))
    }

    /// Calculate `roots::heads`.
    def range(&self, roots: Names, heads: Names) -> PyResult<Names> {
        Ok(Names(self.dag(py).range(roots.0, heads.0).map_pyerr(py)?))
    }

    /// Calculate `reachable % unreachable`.
    def only(&self, reachable: Names, unreachable: Names) -> PyResult<Names> {
        let result = self.dag(py).only(reachable.0, unreachable.0).map_pyerr(py)?;
        Ok(Names(result))
    }

    /// Calculate `reachable % unreachable`, and `::unreachable`.
    def onlyboth(&self, reachable: Names, unreachable: Names) -> PyResult<(Names, Names)> {
        let (reachable_ancestors, unreachable_ancestors) =
            self.dag(py).only_both(reachable.0, unreachable.0).map_pyerr(py)?;
        Ok((Names(reachable_ancestors), Names(unreachable_ancestors)))
    }

    /// Calculate descendants of the given set.
    def descendants(&self, set: Names) -> PyResult<Names> {
        Ok(Names(self.dag(py).descendants(set.0).map_pyerr(py)?))
    }

    /// Calculate `roots & (heads | parents(only(heads, roots & ancestors(heads))))`.
    def reachableroots(&self, roots: Names, heads: Names) -> PyResult<Names> {
        let result = self.dag(py).reachable_roots(roots.0, heads.0).map_pyerr(py)?;
        Ok(Names(result))
    }

    /// Beautify the graph so `render` might look better.
    def beautify(&self, mainbranch: Option<Names> = None) -> PyResult<Self> {
        let dag = self.dag(py).beautify(mainbranch.map(|h| h.0)).map_pyerr(py)?;
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

    /// Inverse the DAG. Swap parents and children.
    def inverse(&self) -> PyResult<Self> {
        let dag = self.dag(py).clone();
        let inversed = InverseDag::new(dag);
        Self::from_dag(py, inversed)
    }
});

impl dagalgo {
    pub fn from_dag(py: Python, dag: impl DagAlgorithm + Send + Sync + 'static) -> PyResult<Self> {
        Self::create_instance(py, Arc::new(dag))
    }
}
