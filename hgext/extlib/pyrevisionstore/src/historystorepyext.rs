use cpython::{PyBytes, PyDict, PyErr, PyIterator, PyList, PyResult, PyTuple, Python, PythonObject,
              ToPyObject};

use pythonutil::{from_key_to_tuple, from_tuple_to_key, to_key, to_pyerr};
use revisionstore::historystore::{HistoryStore, NodeInfo};
use revisionstore::key::Key;

pub trait HistoryStorePyExt {
    fn get_ancestors(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict>;
    fn get_missing(&self, py: Python, keys: &mut PyIterator) -> PyResult<PyList>;
    fn get_node_info(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyTuple>;
}

impl<T: HistoryStore> HistoryStorePyExt for T {
    fn get_ancestors(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict> {
        let key = to_key(py, name, node);
        let ancestors = self.get_ancestors(&key).map_err(|e| to_pyerr(py, &e))?;
        let ancestors = ancestors.iter().map(|(k, v)| {
            (
                PyBytes::new(py, k.node().as_ref()),
                from_node_info(py, k, v),
            )
        });
        let pyancestors = PyDict::new(py);
        for (node, value) in ancestors {
            pyancestors.set_item(py, node, value)?;
        }
        Ok(pyancestors)
    }

    fn get_missing(&self, py: Python, keys: &mut PyIterator) -> PyResult<PyList> {
        unimplemented!()
    }

    fn get_node_info(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyTuple> {
        unimplemented!()
    }
}

fn from_node_info(py: Python, key: &Key, info: &NodeInfo) -> PyTuple {
    (
        PyBytes::new(py, info.parents[0].node().as_ref()),
        PyBytes::new(py, info.parents[1].node().as_ref()),
        PyBytes::new(py, info.linknode.as_ref().as_ref()),
        if key.name() != info.parents[0].name() {
            PyBytes::new(py, info.parents[0].name()).into_object()
        } else {
            Python::None(py)
        },
    ).into_py_object(py)
}
