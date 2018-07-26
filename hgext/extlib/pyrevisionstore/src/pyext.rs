// Copyright Facebook, Inc. 2018
//! Python bindings for a Rust hg store
use cpython::{ObjectProtocol, PyBytes, PyClone, PyDict, PyErr, PyIterator, PyList, PyObject,
              PyResult, Python, PythonObject, ToPyObject};
use std::collections::HashSet;
use std::path::PathBuf;

use pathencoding;
use pythondatastore::PythonDataStore;
use pythonutil::{from_delta_to_tuple, from_key, from_key_to_tuple, from_tuple_to_key, to_key,
                 to_pyerr};
use revisionstore::datapack::DataPack;
use revisionstore::datastore::DataStore;
use revisionstore::key::Key;
use revisionstore::node::Node;
use revisionstore::repack::{RepackOutputType, RepackResult, Repackable};

py_module_initializer!(
    pyrevisionstore,        // module name
    initpyrevisionstore,    // py2 init name
    PyInit_pyrevisionstore, // py3 init name
    |py, m| {
        // init function
        m.add_class::<datastore>(py)?;
        m.add_class::<datapack>(py)?;
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
        let path = pathencoding::local_bytes_to_path(path.data(py))
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
        let path = pathencoding::path_to_local_bytes(self.store(py).base_path())
                                 .map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(PyBytes::new(py, &path))
    }

    def packpath(&self) -> PyResult<PyBytes> {
        let path = pathencoding::path_to_local_bytes(self.store(py).pack_path())
                                 .map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(PyBytes::new(py, &path))
    }

    def indexpath(&self) -> PyResult<PyBytes> {
        let path = pathencoding::path_to_local_bytes(self.store(py).index_path())
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

trait DataStorePyExt {
    fn get(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyBytes>;
    fn get_delta_chain(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyList>;
    fn get_delta(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyObject>;
    fn get_meta(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict>;
    fn get_missing(&self, py: Python, keys: &mut PyIterator) -> PyResult<PyList>;
}

impl<T: DataStore> DataStorePyExt for T {
    fn get(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyBytes> {
        let key = to_key(py, name, node);
        let result = <DataStore>::get(self, &key).map_err(|e| to_pyerr(py, &e))?;

        Ok(PyBytes::new(py, &result[..]))
    }

    fn get_delta(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyObject> {
        let key = to_key(py, name, node);
        let delta = self.get_delta(&key).map_err(|e| to_pyerr(py, &e))?;

        let (base_name, base_node) = if let Some(key) = delta.base {
            from_key(py, &key)
        } else {
            (
                PyBytes::new(py, key.name()),
                PyBytes::new(py, Node::null_id().as_ref()),
            )
        };

        let bytes = PyBytes::new(py, &delta.data);
        let meta = <DataStorePyExt>::get_meta(self, py.clone(), &name, &node)?;
        Ok((
            bytes.into_object(),
            base_name.into_object(),
            base_node.into_object(),
            meta.into_object(),
        ).into_py_object(py)
            .into_object())
    }

    fn get_delta_chain(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyList> {
        let key = to_key(py, name, node);
        let deltachain = self.get_delta_chain(&key).map_err(|e| to_pyerr(py, &e))?;
        let pychain = deltachain
            .iter()
            .map(|d| from_delta_to_tuple(py, &d))
            .collect::<Vec<PyObject>>();
        Ok(PyList::new(py, &pychain[..]))
    }

    fn get_meta(&self, py: Python, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict> {
        let key = to_key(py, name, node);
        let metadata = self.get_meta(&key).map_err(|e| to_pyerr(py, &e))?;
        let metadict = PyDict::new(py);
        if let Some(size) = metadata.size {
            metadict.set_item(py, "s", size)?;
        }
        if let Some(flags) = metadata.flags {
            metadict.set_item(py, "f", flags)?;
        }

        Ok(metadict)
    }

    fn get_missing(&self, py: Python, keys: &mut PyIterator) -> PyResult<PyList> {
        // Copy the PyObjects into a vector so we can get a reference iterator.
        // This lets us get a Vector of Keys without copying the strings.
        let keys = keys.map(|k| match k {
            Ok(k) => from_tuple_to_key(py, &k),
            Err(e) => Err(e),
        }).collect::<Result<Vec<Key>, PyErr>>()?;
        let missing = self.get_missing(&keys[..]).map_err(|e| to_pyerr(py, &e))?;

        let results = PyList::new(py, &[]);
        for key in missing {
            let key_tuple = from_key_to_tuple(py, &key);
            results.insert_item(py, results.len(py), key_tuple.into_object());
        }

        Ok(results)
    }
}

trait RepackablePyExt {
    fn mark_ledger(&self, py: Python, py_store: &PyObject, ledger: &PyObject) -> PyResult<()>;
    fn cleanup(&self, py: Python, ledger: &PyObject) -> PyResult<()>;
}

impl<T: Repackable> RepackablePyExt for T {
    fn mark_ledger(&self, py: Python, py_store: &PyObject, ledger: &PyObject) -> PyResult<()> {
        for entry in self.repack_iter() {
            let (_path, kind, key) = entry.map_err(|e| to_pyerr(py, &e))?;
            let (name, node) = from_key(py, &key);
            let kind = match kind {
                RepackOutputType::Data => "markdataentry",
                RepackOutputType::History => "markhistoryentry",
            };
            ledger.call_method(py, kind, (py_store, name, node).into_py_object(py), None)?;
        }

        Ok(())
    }

    fn cleanup(&self, py: Python, ledger: &PyObject) -> PyResult<()> {
        let py_entries = ledger.getattr(py, "entries")?;
        let packed_entries = py_entries.cast_as::<PyDict>(py)?;

        let mut repacked: HashSet<Key> = HashSet::with_capacity(packed_entries.len(py));

        for &(ref key, ref entry) in packed_entries.items(py).iter() {
            let key = from_tuple_to_key(py, &key)?;
            if entry.getattr(py, "datarepacked")?.is_true(py)?
                || entry.getattr(py, "gced")?.is_true(py)?
            {
                repacked.insert(key);
            }
        }

        let created = ledger.getattr(py, "created")?;
        let created: HashSet<PathBuf> = created
            .iter(py)?
            .map(|py_name| {
                let py_name = py_name?;
                Ok(PathBuf::from(pathencoding::local_bytes_to_path(
                    py_name.cast_as::<PyBytes>(py)?.data(py),
                ).map_err(|e| {
                    to_pyerr(py, &e.into())
                })?))
            })
            .collect::<Result<HashSet<PathBuf>, PyErr>>()?;

        self.cleanup(&RepackResult::new(repacked, created))
            .map_err(|e| to_pyerr(py, &e))?;
        Ok(())
    }
}
