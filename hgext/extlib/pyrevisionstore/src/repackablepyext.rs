use cpython::{ObjectProtocol, PyBytes, PyDict, PyErr, PyObject, PyResult, Python, ToPyObject};
use std::collections::HashSet;
use std::path::PathBuf;

use pathencoding;
use pythonutil::{from_key, from_tuple_to_key, to_pyerr};
use revisionstore::key::Key;
use revisionstore::repack::{RepackOutputType, RepackResult, Repackable};

pub trait RepackablePyExt {
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
