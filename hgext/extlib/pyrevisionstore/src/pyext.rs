// Copyright Facebook, Inc. 2018
//! Python bindings for a Rust hg store

use cpython::{PyBytes, PyClone, PyDict, PyErr, PyList, PyObject, PyResult, PythonObject};
use pythondatastore::PythonDataStore;
use pythonutil::{from_delta_to_tuple, from_key_to_tuple, from_tuple_to_key, to_key, to_pyerr};
use revisionstore::datastore::DataStore;
use revisionstore::key::Key;

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
        let deltachain = self.store(py).get_delta_chain(&key)
                                       .map_err(|e| to_pyerr(py, &e))?;
        let pychain = deltachain.iter()
                                .map(|d| from_delta_to_tuple(py, &d))
                                .collect::<Vec<PyObject>>();
        Ok(PyList::new(py, &pychain[..]))
    }

    def getmeta(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict> {
        let key = to_key(py, name, node);
        let metadata = self.store(py).get_meta(&key)
                                     .map_err(|e| to_pyerr(py, &e))?;
        let metadict = PyDict::new(py);
        if let Some(size) = metadata.size {
            metadict.set_item(py, "s", size)?;
        }
        if let Some(flags) = metadata.flags {
            metadict.set_item(py, "f", flags)?;
        }

        Ok(metadict)
    }

    def getmissing(&self, keys: &PyList) -> PyResult<PyList> {
        // Copy the PyObjects into a vector so we can get a reference iterator.
        // This lets us get a Vector of Keys without copying the strings.
        let keys = keys.iter(py)
                       .map(|k| from_tuple_to_key(py, &k))
                       .collect::<Result<Vec<Key>, PyErr>>()?;
        let missing = self.store(py).get_missing(&keys[..])
                                    .map_err(|e| to_pyerr(py, &e))?;

        let results = PyList::new(py, &[]);
        for key in missing {
            let key_tuple = from_key_to_tuple(py, &key);
            results.insert_item(py, results.len(py), key_tuple.into_object());
        }

        Ok(results)
    }
});
