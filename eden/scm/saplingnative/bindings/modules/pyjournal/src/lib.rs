/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use cpython::*;
use cpython_ext::convert::Serde;
use cpython_ext::ResultPyErrExt;
use journal::JournalEntry;
use types::HgId;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "journal"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<journalentry>(py)?;
    Ok(m)
}

py_class!(class journalentry |py| {
    data inner: JournalEntry;

    def __new__(
        _cls,
        timestamp: &PyTuple,
        user: String,
        command: String,
        namespace: String,
        name: String,
        old_hashes: Serde<Vec<HgId>>,
        new_hashes: Serde<Vec<HgId>>,
    ) -> PyResult<journalentry> {
        let timestamp = timestamp.as_slice(py);
        let (unixtime, offset) = (<f64>::extract(py, &timestamp[0])? as i64, <i32>::extract(py, &timestamp[1])?);
        let timestamp = hgtime::HgTime {unixtime, offset}.bounded().expect("unable to determine time");
        Self::create_instance(
            py,
            JournalEntry {
                timestamp,
                user,
                command,
                namespace,
                name,
                old_hashes: old_hashes.0,
                new_hashes: new_hashes.0,
            },
        )
    }

    @classmethod def fromstorage(_cls, line: PyBytes) -> PyResult<journalentry> {
        Self::create_instance(
            py,
            JournalEntry::deserialize(line.data(py)).map_pyerr(py)?,
        )
    }

    def serialize(&self) -> PyResult<PyBytes> {
        let mut bytes_entry = Vec::new();
        self.inner(py).serialize(&mut bytes_entry).map_pyerr(py)?;
        Ok(PyBytes::new(py, bytes_entry.as_ref()))
    }

    @property def timestamp(&self) -> PyResult<PyTuple> {
        let timestamp = self.inner(py).timestamp;
        Ok(PyTuple::new(py, &[
            timestamp.unixtime.to_py_object(py).into_object(),
            timestamp.offset.to_py_object(py).into_object()
        ]))
    }

    @property def user(&self) -> PyResult<String> {
        Ok(self.inner(py).user.clone())
    }

    @property def command(&self) -> PyResult<String> {
        Ok(self.inner(py).command.clone())
    }

    @property def namespace(&self) -> PyResult<String> {
        Ok(self.inner(py).namespace.clone())
    }

    @property def name(&self) -> PyResult<String> {
        Ok(self.inner(py).name.clone())
    }

    @property def oldhashes(&self) -> PyResult<Serde<Vec<HgId>>> {
        Ok(Serde(self.inner(py).old_hashes.clone()))
    }

    @property def newhashes(&self) -> PyResult<Serde<Vec<HgId>>> {
        Ok(Serde(self.inner(py).new_hashes.clone()))
    }
});
