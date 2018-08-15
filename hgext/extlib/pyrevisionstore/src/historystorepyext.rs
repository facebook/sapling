use cpython::{PyBytes, PyDict, PyErr, PyIterator, PyList, PyObject, PyResult, Python,
              PythonObject, ToPyObject};

use pythonutil::{from_delta_to_tuple, from_key, from_key_to_tuple, from_tuple_to_key, to_key,
                 to_pyerr};
use revisionstore::historystore::HistoryStore;
use revisionstore::key::Key;

pub trait HistoryStorePyExt {
    fn get_ancestors(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict>;
    fn get_missing(&self, py: Python, keys: &mut PyIterator) -> PyResult<PyList>;
    fn get_node_info(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyTuple>;
}

impl<T: HistoryStore> HistoryStorePyExt for T {
    fn get_ancestors(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyList> {
        unimplemented!()
    }

    fn get_missing(&self, py: Python, keys: &mut PyIterator) -> PyResult<PyList> {
        unimplemented!()
    }

    fn get_node_info(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyTuple> {
        unimplemented!()
    }
}
