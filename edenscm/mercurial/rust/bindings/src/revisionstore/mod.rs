// Copyright Facebook, Inc. 2018
//! revisionstore - Python interop layer for a Mercurial data and history store

#![allow(non_camel_case_types)]

use std::path::PathBuf;

use cpython::*;
use failure::Error;

use encoding;
use revisionstore::{
    repack::{filter_incrementalpacks, list_packs, repack_datapacks, repack_historypacks},
    DataPack, HistoryPack, IndexedLogDataStore, MutableDeltaStore,
};

use crate::revisionstore::datastorepyext::DataStorePyExt;
use crate::revisionstore::historystorepyext::HistoryStorePyExt;
use crate::revisionstore::pyext::PyOptionalRefCell;
use crate::revisionstore::pythondatastore::PythonDataStore;
use crate::revisionstore::pythonutil::to_pyerr;
use crate::revisionstore::repackablepyext::RepackablePyExt;

mod datastorepyext;
mod historystorepyext;
mod pyerror;
mod pyext;
mod pythondatastore;
mod pythonhistorystore;
mod pythonutil;
mod repackablepyext;

pub use crate::revisionstore::pythondatastore::PythonMutableDataPack;
pub use crate::revisionstore::pythonhistorystore::PythonMutableHistoryPack;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "revisionstore"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<datastore>(py)?;
    m.add_class::<datapack>(py)?;
    m.add_class::<historypack>(py)?;
    m.add_class::<indexedlogdatastore>(py)?;
    m.add(
        py,
        "repackdatapacks",
        py_fn!(py, repackdata(packpath: PyBytes, outdir: PyBytes)),
    )?;
    m.add(
        py,
        "repackincrementaldatapacks",
        py_fn!(
            py,
            incremental_repackdata(packpath: PyBytes, outdir: PyBytes)
        ),
    )?;
    m.add(
        py,
        "repackhistpacks",
        py_fn!(py, repackhist(packpath: PyBytes, outdir: PyBytes)),
    )?;
    m.add(
        py,
        "repackincrementalhistpacks",
        py_fn!(
            py,
            incremental_repackhist(packpath: PyBytes, outdir: PyBytes)
        ),
    )?;
    Ok(m)
}

/// Helper function to de-serialize and re-serialize from and to Python objects.
fn repack_pywrapper(
    py: Python,
    packpath: PyBytes,
    outdir_py: PyBytes,
    repacker: impl FnOnce(PathBuf, PathBuf) -> Result<PathBuf, Error>,
) -> PyResult<PyBytes> {
    let path =
        encoding::local_bytes_to_path(packpath.data(py)).map_err(|e| to_pyerr(py, &e.into()))?;

    let outdir =
        encoding::local_bytes_to_path(outdir_py.data(py)).map_err(|e| to_pyerr(py, &e.into()))?;
    repacker(path.to_path_buf(), outdir.to_path_buf())
        .and_then(|p| Ok(PyBytes::new(py, &encoding::path_to_local_bytes(&p)?)))
        .map_err(|e| to_pyerr(py, &e.into()))
}

/// Merge all the datapacks into one big datapack. Returns the fullpath of the resulting datapack.
fn repackdata(py: Python, packpath: PyBytes, outdir_py: PyBytes) -> PyResult<PyBytes> {
    repack_pywrapper(py, packpath, outdir_py, |dir, outdir| {
        repack_datapacks(list_packs(&dir, "datapack")?.iter(), &outdir)
    })
}

/// Merge all the history packs into one big historypack. Returns the fullpath of the resulting
/// histpack.
fn repackhist(py: Python, packpath: PyBytes, outdir_py: PyBytes) -> PyResult<PyBytes> {
    repack_pywrapper(py, packpath, outdir_py, |dir, outdir| {
        repack_historypacks(list_packs(&dir, "histpack")?.iter(), &outdir)
    })
}

/// Perform an incremental repack of data packs.
fn incremental_repackdata(py: Python, packpath: PyBytes, outdir_py: PyBytes) -> PyResult<PyBytes> {
    repack_pywrapper(py, packpath, outdir_py, |dir, outdir| {
        repack_datapacks(
            filter_incrementalpacks(list_packs(&dir, "datapack")?, "datapack")?.iter(),
            &outdir,
        )
    })
}

/// Perform an incremental repack of history packs.
fn incremental_repackhist(py: Python, packpath: PyBytes, outdir_py: PyBytes) -> PyResult<PyBytes> {
    repack_pywrapper(py, packpath, outdir_py, |dir, outdir| {
        repack_historypacks(
            filter_incrementalpacks(list_packs(&dir, "histpack")?, "histpack")?.iter(),
            &outdir,
        )
    })
}

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
        self.store(py).get_py(py, name, node)
    }

    def getdeltachain(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyList> {
        self.store(py).get_delta_chain_py(py, name, node)
    }

    def getmeta(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict> {
        self.store(py).get_meta_py(py, name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        self.store(py).get_missing_py(py, &mut keys.iter(py)?)
    }
});
py_class!(class datapack |py| {
    data store: PyOptionalRefCell<Box<DataPack>>;

    def __new__(
        _cls,
        path: &PyBytes
    ) -> PyResult<datapack> {
        let path = encoding::local_bytes_to_path(path.data(py))
                                 .map_err(|e| to_pyerr(py, &e.into()))?;
        datapack::create_instance(
            py,
            PyOptionalRefCell::new(Box::new(match DataPack::new(&path) {
                Ok(pack) => pack,
                Err(e) => return Err(to_pyerr(py, &e)),
            })),
        )
    }

    def path(&self) -> PyResult<PyBytes> {
        let store = self.store(py).get_value(py)?;
        let path = encoding::path_to_local_bytes(store.base_path()).map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(PyBytes::new(py, &path))
    }

    def packpath(&self) -> PyResult<PyBytes> {
        let store = self.store(py).get_value(py)?;
        let path = encoding::path_to_local_bytes(store.pack_path()).map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(PyBytes::new(py, &path))
    }

    def indexpath(&self) -> PyResult<PyBytes> {
        let store = self.store(py).get_value(py)?;
        let path = encoding::path_to_local_bytes(store.index_path()).map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(PyBytes::new(py, &path))
    }

    def get(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyBytes> {
        let store = self.store(py).get_value(py)?;
        store.get_py(py, name, node)
    }

    def getdelta(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyObject> {
        let store = self.store(py).get_value(py)?;
        store.get_delta_py(py, name, node)
    }

    def getdeltachain(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyList> {
        let store = self.store(py).get_value(py)?;
        store.get_delta_chain_py(py, name, node)
    }

    def getmeta(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict> {
        let store = self.store(py).get_value(py)?;
        store.get_meta_py(py, name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        let store = self.store(py).get_value(py)?;
        store.get_missing_py(py, &mut keys.iter(py)?)
    }

    def markledger(&self, ledger: &PyObject, _options: &PyObject) -> PyResult<PyObject> {
        let store = self.store(py).get_value(py)?;
        store.mark_ledger(py, self.as_object(), ledger)?;
        Ok(Python::None(py))
    }

    def cleanup(&self, ledger: &PyObject) -> PyResult<PyObject> {
        let datapack = self.store(py).take_value(py)?;
        datapack.cleanup(py, ledger)?;
        Ok(Python::None(py))
    }
});

py_class!(class historypack |py| {
    data store: PyOptionalRefCell<Box<HistoryPack>>;

    def __new__(
        _cls,
        path: &PyBytes
    ) -> PyResult<historypack> {
        let path = encoding::local_bytes_to_path(path.data(py))
                                 .map_err(|e| to_pyerr(py, &e.into()))?;
        historypack::create_instance(
            py,
            PyOptionalRefCell::new(Box::new(match HistoryPack::new(&path) {
                Ok(pack) => pack,
                Err(e) => return Err(to_pyerr(py, &e)),
            })),
        )
    }

    def path(&self) -> PyResult<PyBytes> {
        let store = self.store(py).get_value(py)?;
        let path = encoding::path_to_local_bytes(store.base_path()).map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(PyBytes::new(py, &path))
    }

    def packpath(&self) -> PyResult<PyBytes> {
        let store = self.store(py).get_value(py)?;
        let path = encoding::path_to_local_bytes(store.pack_path()).map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(PyBytes::new(py, &path))
    }

    def indexpath(&self) -> PyResult<PyBytes> {
        let store = self.store(py).get_value(py)?;
        let path = encoding::path_to_local_bytes(store.index_path()).map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(PyBytes::new(py, &path))
    }

    def getancestors(&self, name: &PyBytes, node: &PyBytes, known: Option<&PyObject>) -> PyResult<PyDict> {
        let _known = known;
        let store = self.store(py).get_value(py)?;
        store.get_ancestors_py(py, name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        let store = self.store(py).get_value(py)?;
        store.get_missing_py(py, &mut keys.iter(py)?)
    }

    def getnodeinfo(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyTuple> {
        let store = self.store(py).get_value(py)?;
        store.get_node_info_py(py, name, node)
    }

    def markledger(&self, ledger: &PyObject, _options: &PyObject) -> PyResult<PyObject> {
        let store = self.store(py).get_value(py)?;
        store.mark_ledger(py, self.as_object(), ledger)?;
        Ok(Python::None(py))
    }

    def cleanup(&self, ledger: &PyObject) -> PyResult<PyObject> {
        let historypack = self.store(py).take_value(py)?;
        historypack.cleanup(py, ledger)?;
        Ok(Python::None(py))
    }
});

py_class!(class indexedlogdatastore |py| {
    data store: PyOptionalRefCell<Box<IndexedLogDataStore>>;

    def __new__(_cls, path: &PyBytes) -> PyResult<indexedlogdatastore> {
        let path = encoding::local_bytes_to_path(path.data(py))
                                 .map_err(|e| to_pyerr(py, &e.into()))?;
        indexedlogdatastore::create_instance(
            py,
            PyOptionalRefCell::new(Box::new(match IndexedLogDataStore::new(&path) {
                Ok(log) => log,
                Err(e) => return Err(to_pyerr(py, &e)),
            })),
        )
    }

    def getdelta(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyObject> {
        let store = self.store(py).get_value(py)?;
        store.get_delta_py(py, name, node)
    }

    def getdeltachain(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyList> {
        let store = self.store(py).get_value(py)?;
        store.get_delta_chain_py(py, name, node)
    }

    def getmeta(&self, name: &PyBytes, node: &PyBytes) -> PyResult<PyDict> {
        let store = self.store(py).get_value(py)?;
        store.get_meta_py(py, name, node)
    }

    def getmissing(&self, keys: &PyObject) -> PyResult<PyList> {
        let store = self.store(py).get_value(py)?;
        store.get_missing_py(py, &mut keys.iter(py)?)
    }

    def markledger(&self, _ledger: &PyObject, _options: &PyObject) -> PyResult<PyObject> {
        Ok(Python::None(py))
    }

    def markforrefresh(&self) -> PyResult<PyObject> {
        let mut store = self.store(py).get_mut_value(py)?;
        store.flush().map_err(|e| to_pyerr(py, &e.into()))?;
        Ok(Python::None(py))
    }
});
