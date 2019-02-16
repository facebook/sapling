// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cpython::{ObjectProtocol, PyBytes, PyDict, PyErr, PyObject, PyResult, Python, ToPyObject};
use std::collections::HashSet;
use std::path::PathBuf;

use revisionstore::repack::{RepackOutputType, RepackResult, Repackable};
use types::Key;

use encoding;
use pythonutil::{from_key, from_tuple_to_key, to_pyerr};

pub trait RepackablePyExt {
    fn mark_ledger(&self, py: Python, py_store: &PyObject, ledger: &PyObject) -> PyResult<()>;
    fn cleanup(self, py: Python, ledger: &PyObject) -> PyResult<()>;
}

impl<T: Repackable> RepackablePyExt for T {
    fn mark_ledger(&self, py: Python, py_store: &PyObject, ledger: &PyObject) -> PyResult<()> {
        let path = encoding::path_to_local_bytes(self.id()).map_err(|e| to_pyerr(py, &e.into()))?;
        ledger.call_method(py, "setlocation", (PyBytes::new(py, &path),), None)?;
        for entry in self.repack_iter() {
            let (_path, kind, key) = entry.map_err(|e| to_pyerr(py, &e))?;
            let (name, node) = from_key(py, &key);
            let kind = match kind {
                RepackOutputType::Data => "markdataentry",
                RepackOutputType::History => "markhistoryentry",
            };
            ledger.call_method(py, kind, (py_store, name, node).into_py_object(py), None)?;
        }
        ledger.call_method(py, "setlocation", (py.None(),), None)?;

        Ok(())
    }

    fn cleanup(self, py: Python, ledger: &PyObject) -> PyResult<()> {
        let py_entries = ledger.getattr(py, "entries")?;
        let packed_entries = py_entries.cast_as::<PyDict>(py)?;

        let mut repacked: HashSet<Key> = HashSet::with_capacity(packed_entries.len(py));

        for &(ref key, ref entry) in packed_entries.items(py).iter() {
            let key = from_tuple_to_key(py, &key)?;
            let leader_string = match self.kind() {
                RepackOutputType::Data => "datarepacked",
                RepackOutputType::History => "historyrepacked",
            };
            if entry.getattr(py, leader_string)?.is_true(py)?
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
                Ok(PathBuf::from(encoding::local_bytes_to_path(
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
