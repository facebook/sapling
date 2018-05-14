// Copyright Facebook, Inc. 2018
//! Python bindings for a Rust hg store

use cpython::{PyBytes, PyClone, PyList, PyObject, PyResult};
use pythondatastore::PythonDataStore;
use pythonutil::{from_delta_to_tuple, to_key, to_pyerr};
use revisionstore::datastore::DataStore;

py_module_initializer!(
    pyrevisionstore,        // module name
    initpyrevisionstore,    // py2 init name
    PyInit_pyrevisionstore, // py3 init name
    |py, m| {
        // init function
        m.add_class::<datastore>(py)?;
        Ok(())
    }
);

py_class!(class datastore |py| {
    data store: Box<DataStore + Send>;

    def __new__(
        _cls,
        store: &PyObject
    ) -> PyResult<datastore> {
        datastore::create_instance(
            py,
            Box::new(PythonDataStore::new(store.clone_ref(py))),
        )
    }

    def get(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyBytes> {
        let key = to_key(py, name, node);
        let result = self.store(py).get(&key)
                                   .map_err(|e| to_pyerr(py, &e))?;

        Ok(PyBytes::new(py, &result[..]))
    }

    def getdeltachain(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyList> {
        let key = to_key(py, name, node);
        let deltachain = self.store(py).getdeltachain(&key)
                                       .map_err(|e| to_pyerr(py, &e))?;
        let pychain = deltachain.iter()
                                .map(|d| from_delta_to_tuple(py, &d))
                                .collect::<Vec<PyObject>>();
        Ok(PyList::new(py, &pychain[..]))
    }
});
