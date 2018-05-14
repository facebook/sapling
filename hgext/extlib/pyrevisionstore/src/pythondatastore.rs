use cpython::{FromPyObject, ObjectProtocol, PyBytes, PyObject, Python};
use pyerror::pyerr_to_error;
use revisionstore::datastore::DataStore;
use revisionstore::error::Result;
use revisionstore::key::Key;

pub struct PythonDataStore {
    py_store: PyObject,
}

impl PythonDataStore {
    pub fn new(py_store: PyObject) -> Self {
        PythonDataStore { py_store }
    }
}

// All accesses are protected by the GIL, so it's thread safe. This is required because it is
// eventually stored on the `datastore` python class and Rust CPython requires that stored members
// implement Send.
unsafe impl Send for PythonDataStore {}

impl DataStore for PythonDataStore {
    fn get(&self, key: &Key) -> Result<Vec<u8>> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let py_name = PyBytes::new(py, key.name());
        let py_node = PyBytes::new(py, key.node().as_ref());

        let py_data = self.py_store
            .call_method(py, "get", (py_name, py_node), None)
            .map_err(|e| pyerr_to_error(py, e))?;

        let py_bytes = PyBytes::extract(py, &py_data).map_err(|e| pyerr_to_error(py, e))?;

        Ok(py_bytes.data(py).to_vec())
    }
}
