// Copyright Facebook, Inc. 2018
//! Python bindings for a Rust hg store
use cpython::{ObjectProtocol, PyBytes, PyClone, PyDict, PyList, PyObject, PyResult, PyTuple,
              Python, PythonObject};
use encoding;

use datastorepyext::DataStorePyExt;
use historystorepyext::HistoryStorePyExt;
use pythondatastore::PythonDataStore;
use pythonutil::to_pyerr;
use repackablepyext::RepackablePyExt;
use revisionstore::datapack::DataPack;
use revisionstore::historypack::HistoryPack;

py_module_initializer!(
    pyrevisionstore,        // module name
    initpyrevisionstore,    // py2 init name
    PyInit_pyrevisionstore, // py3 init name
    |py, m| {
        // init function
        m.add_class::<datastore>(py)?;
        m.add_class::<datapack>(py)?;
        m.add_class::<historypack>(py)?;
        Ok(())
    }
);

py_class!(class datastore |py| {
    data store: Box<DataStorePyExt + Send>;

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
        self.store(py).get(py, name, node)
    }

    def getdeltachain(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyList> {
        self.store(py).get_delta_chain(py, name, node)
    }

    def getmeta(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict> {
        self.store(py).get_meta(py, name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        self.store(py).get_missing(py, &mut keys.iter(py)?)
    }
});

py_class!(class datapack |py| {
    data store: Box<DataPack>;

    def __new__(
        _cls,
        path: &PyBytes
    ) -> PyResult<datapack> {
        let path = encoding::local_bytes_to_path(path.data(py))
                                 .map_err(|e| to_pyerr(py, &e.into()))?;
        datapack::create_instance(
            py,
            Box::new(match DataPack::new(&path) {
                Ok(pack) => pack,
                Err(e) => return Err(to_pyerr(py, &e)),
            }),
        )
    }

    def path(&self) -> PyResult<PyBytes> {
        let path = encoding::path_to_local_bytes(self.store(py).base_path())
                                 .map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(PyBytes::new(py, &path))
    }

    def packpath(&self) -> PyResult<PyBytes> {
        let path = encoding::path_to_local_bytes(self.store(py).pack_path())
                                 .map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(PyBytes::new(py, &path))
    }

    def indexpath(&self) -> PyResult<PyBytes> {
        let path = encoding::path_to_local_bytes(self.store(py).index_path())
                                 .map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(PyBytes::new(py, &path))
    }

    def get(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyBytes> {
        <DataStorePyExt>::get(self.store(py).as_ref(), py, name, node)
    }

    def getdelta(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyObject> {
        <DataStorePyExt>::get_delta(self.store(py).as_ref(), py, name, node)
    }

    def getdeltachain(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyList> {
        <DataStorePyExt>::get_delta_chain(self.store(py).as_ref(), py, name, node)
    }

    def getmeta(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict> {
        <DataStorePyExt>::get_meta(self.store(py).as_ref(), py, name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        <DataStorePyExt>::get_missing(self.store(py).as_ref(), py, &mut keys.iter(py)?)
    }

    def markledger(&self, ledger: &PyObject, _options: &PyObject) -> PyResult<PyObject> {
        <RepackablePyExt>::mark_ledger(self.store(py).as_ref(), py, self.as_object(), ledger)?;
        Ok(Python::None(py))
    }

    def cleanup(&self, ledger: &PyObject) -> PyResult<PyObject> {
        <RepackablePyExt>::cleanup(self.store(py).as_ref(), py, ledger)?;
        Ok(Python::None(py))
    }
});

py_class!(class historypack |py| {
    data store: Box<HistoryPack>;

    def __new__(
        _cls,
        path: &PyBytes
    ) -> PyResult<historypack> {
        let path = encoding::local_bytes_to_path(path.data(py))
                                 .map_err(|e| to_pyerr(py, &e.into()))?;
        historypack::create_instance(
            py,
            Box::new(HistoryPack::new(&path).map_err(|e| to_pyerr(py, &e))?),
        )
    }

    def path(&self) -> PyResult<PyBytes> {
        let path = encoding::path_to_local_bytes(self.store(py).base_path())
                                 .map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(PyBytes::new(py, &path))
    }

    def packpath(&self) -> PyResult<PyBytes> {
        let path = encoding::path_to_local_bytes(self.store(py).pack_path())
                                 .map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(PyBytes::new(py, &path))
    }

    def indexpath(&self) -> PyResult<PyBytes> {
        let path = encoding::path_to_local_bytes(self.store(py).index_path())
                                 .map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(PyBytes::new(py, &path))
    }

    def getancestors(&self, name: &PyBytes, node: &PyBytes, known: Option<&PyObject>) -> PyResult<PyDict> {
        <HistoryStorePyExt>::get_ancestors(self.store(py).as_ref(), py, name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        <HistoryStorePyExt>::get_missing(self.store(py).as_ref(), py, &mut keys.iter(py)?)
    }

    def getnodeinfo(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyTuple> {
        <HistoryStorePyExt>::get_node_info(self.store(py).as_ref(), py, name, node)
    }

    def markledger(&self, ledger: &PyObject, _options: &PyObject) -> PyResult<PyObject> {
        <RepackablePyExt>::mark_ledger(self.store(py).as_ref(), py, self.as_object(), ledger)?;
        Ok(Python::None(py))
    }

    def cleanup(&self, ledger: &PyObject) -> PyResult<PyObject> {
        <RepackablePyExt>::cleanup(self.store(py).as_ref(), py, ledger)?;
        Ok(Python::None(py))
    }
});
