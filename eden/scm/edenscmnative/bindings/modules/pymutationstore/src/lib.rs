/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::{cell::RefCell, io::Cursor};

use anyhow::Error;
use byteorder::{ReadBytesExt, WriteBytesExt};
use cpython::*;
use cpython_failure::ResultPyErrExt;
use thiserror::Error;

use ::mutationstore::{MutationEntry, MutationEntryOrigin, MutationStore, Repair};
use encoding::local_bytes_to_path;
use types::node::Node;
use vlqencoding::{VLQDecode, VLQEncode};

/// Supported format of bundle version.
/// Format 1 is:
///  * Single byte version: 0x01
///  * VLQ-encoded count of entries: ``count``
///  * A sequence of ``count`` entries encoded using ``MutationEntry::serialize``
const BUNDLE_FORMAT_VERSION: u8 = 1u8;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "mutationstore"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "ORIGIN_COMMIT", ::mutationstore::ORIGIN_COMMIT)?;
    m.add(py, "ORIGIN_OBSMARKER", ::mutationstore::ORIGIN_OBSMARKER)?;
    m.add(py, "ORIGIN_SYNTHETIC", ::mutationstore::ORIGIN_SYNTHETIC)?;
    m.add(py, "ORIGIN_LOCAL", ::mutationstore::ORIGIN_LOCAL)?;
    m.add_class::<mutationentry>(py)?;
    m.add_class::<mutationstore>(py)?;
    m.add(
        py,
        "bundle",
        py_fn!(py, bundle(entries: Vec<mutationentry>)),
    )?;
    m.add(py, "unbundle", py_fn!(py, unbundle(data: PyBytes)))?;
    Ok(m)
}

fn bundle(py: Python, entries: Vec<mutationentry>) -> PyResult<PyBytes> {
    // Pre-allocate capacity for all the entries, plus one for the header and extra breathing room.
    let mut buf = Vec::with_capacity((entries.len() + 1) * ::mutationstore::DEFAULT_ENTRY_SIZE);
    buf.write_u8(BUNDLE_FORMAT_VERSION)
        .map_pyerr::<exc::IOError>(py)?;
    buf.write_vlq(entries.len()).map_pyerr::<exc::IOError>(py)?;
    for entry in entries {
        let entry = entry.entry(py);
        entry.serialize(&mut buf).map_pyerr::<exc::IOError>(py)?;
    }
    Ok(PyBytes::new(py, &buf))
}

fn unbundle(py: Python, data: PyBytes) -> PyResult<Vec<mutationentry>> {
    let mut cursor = Cursor::new(data.data(py));
    let version = cursor.read_u8().map_pyerr::<exc::IOError>(py)?;
    if version != BUNDLE_FORMAT_VERSION {
        return Err(PyErr::new::<exc::IOError, _>(
            py,
            format!("Unsupported mutation format: {}", version),
        ));
    }
    let count = cursor.read_vlq().map_pyerr::<exc::IOError>(py)?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let entry = MutationEntry::deserialize(&mut cursor).map_pyerr::<exc::IOError>(py)?;
        entries.push(mutationentry::create_instance(py, entry)?);
    }
    Ok(entries)
}

#[derive(Error, Debug)]
enum InvalidNode {
    #[error("Invalid successor node: {0}")]
    Successor(Error),
    #[error("Invalid predecessor node: {0}")]
    Predecessor(Error),
    #[error("Invalid split node: {0}")]
    Split(Error),
}

py_class!(class mutationentry |py| {
    data entry: MutationEntry;

    def __new__(
        _cls,
        origin: u8,
        succ: &PyBytes,
        preds: Option<Vec<PyBytes>>,
        split: Option<Vec<PyBytes>>,
        op: &PyString,
        user: &PyBytes,
        time: f64,
        tz: i32,
        extra: Option<Vec<(PyBytes, PyBytes)>>
    ) -> PyResult<mutationentry> {
        let origin = MutationEntryOrigin::from_id(origin).map_pyerr::<exc::ValueError>(py)?;
        let succ = Node::from_slice(succ.data(py))
            .map_err(InvalidNode::Successor)
            .map_pyerr::<exc::ValueError>(py)?;
        let preds = {
            let mut nodes = Vec::new();
            if let Some(preds) = preds {
                for p in preds {
                    nodes.push(Node::from_slice(p.data(py))
                        .map_err(InvalidNode::Predecessor)
                        .map_pyerr::<exc::ValueError>(py)?);
                }
            }
            nodes
        };
        let split = {
            let mut nodes = Vec::new();
            if let Some(split) = split {
                for s in split {
                    nodes.push(Node::from_slice(s.data(py))
                        .map_err(InvalidNode::Split)
                        .map_pyerr::<exc::ValueError>(py)?);
                }
            }
            nodes
        };
        let op = op.to_string(py)?.into();
        let user = Box::from(user.data(py));
        let extra = {
            let mut items = Vec::new();
            if let Some(extra) = extra {
                for (k, v) in extra {
                    items.push((Box::from(k.data(py)), Box::from(v.data(py))));
                }
            }
            items
        };
        mutationentry::create_instance(py, MutationEntry {
            origin, succ, preds, split, op, user, time, tz, extra
        })
    }

    def origin(&self) -> PyResult<u8> {
        Ok(self.entry(py).origin.get_id())
    }

    def succ(&self) -> PyResult<PyBytes> {
        Ok(PyBytes::new(py, self.entry(py).succ.as_ref()))
    }

    def preds(&self) -> PyResult<Vec<PyBytes>> {
        Ok(self.entry(py).preds.iter().map(|p| PyBytes::new(py, p.as_ref())).collect())
    }

    def split(&self) -> PyResult<Vec<PyBytes>> {
        Ok(self.entry(py).split.iter().map(|s| PyBytes::new(py, s.as_ref())).collect())
    }

    def op(&self) -> PyResult<PyString> {
        Ok(PyString::new(py, self.entry(py).op.as_ref()))
    }

    def user(&self) -> PyResult<PyBytes> {
        Ok(PyBytes::new(py, self.entry(py).user.as_ref()))
    }

    def time(&self) -> PyResult<f64> {
        Ok(self.entry(py).time)
    }

    def tz(&self) -> PyResult<i32> {
        Ok(self.entry(py).tz)
    }

    def extra(&self) -> PyResult<Vec<(PyBytes, PyBytes)>> {
        Ok(self.entry(py).extra.iter().map(|(k, v)| {
            (PyBytes::new(py, k.as_ref()), PyBytes::new(py, v.as_ref()))
        }).collect())
    }

    def tostoreentry(&self) -> PyResult<mutationentry> {
        Ok(self.to_py_object(py))
    }
});

py_class!(class mutationstore |py| {
    data mut_store: RefCell<MutationStore>;

    def __new__(_cls, path: &PyBytes) -> PyResult<mutationstore> {
        let path = local_bytes_to_path(path.data(py))
            .map_pyerr::<exc::ValueError>(py)?;
        let ms = MutationStore::open(path).map_pyerr::<exc::ValueError>(py)?;
        mutationstore::create_instance(py, RefCell::new(ms))
    }

    def add(&self, entry: &mutationentry) -> PyResult<PyObject> {
        let mut ms = self.mut_store(py).borrow_mut();
        ms.add(entry.entry(py)).map_pyerr::<exc::ValueError>(py)?;
        Ok(py.None())
    }

    def flush(&self) -> PyResult<PyObject> {
        let mut ms = self.mut_store(py).borrow_mut();
        ms.flush().map_pyerr::<exc::ValueError>(py)?;
        Ok(py.None())
    }

    def has(&self, succ: &PyBytes) -> PyResult<bool> {
        let succ = Node::from_slice(succ.data(py)).map_pyerr::<exc::ValueError>(py)?;
        let ms = self.mut_store(py).borrow();
        let entry = ms.get(succ).map_pyerr::<exc::IOError>(py)?;
        Ok(entry.is_some())
    }

    def get(&self, succ: &PyBytes) -> PyResult<Option<mutationentry>> {
        let succ = Node::from_slice(succ.data(py)).map_pyerr::<exc::ValueError>(py)?;
        let ms = self.mut_store(py).borrow();
        let entry = ms.get(succ).map_pyerr::<exc::IOError>(py)?;
        let entry = match entry {
            Some(entry) => Some(mutationentry::create_instance(py, entry)?),
            None => None,
        };
        Ok(entry)
    }

    def getsplithead(&self, node: &PyBytes) -> PyResult<Option<PyBytes>> {
        let node = Node::from_slice(node.data(py)).map_pyerr::<exc::ValueError>(py)?;
        let ms = self.mut_store(py).borrow();
        let entry = ms.get_split_head(node).map_pyerr::<exc::IOError>(py)?;
        let succ = match entry {
            Some(entry) => Some(PyBytes::new(py, entry.succ.as_ref())),
            None => None,
        };
        Ok(succ)
    }

    def getsuccessorssets(&self, node: &PyBytes) -> PyResult<Vec<Vec<PyBytes>>> {
        let node = Node::from_slice(node.data(py)).map_pyerr::<exc::ValueError>(py)?;
        let ms = self.mut_store(py).borrow();
        let ssets = ms.get_successors_sets(node).map_pyerr::<exc::IOError>(py)?;
        let mut pyssets = Vec::with_capacity(ssets.len());
        for sset in ssets.into_iter() {
            let mut pysset = Vec::with_capacity(sset.len());
            for succ in sset.into_iter() {
                pysset.push(PyBytes::new(py, succ.as_ref()));
            }
            pyssets.push(pysset);
        }
        Ok(pyssets)
    }

    @staticmethod
    def repair(path: &str) -> PyResult<PyUnicode> {
        MutationStore::repair(path).map_pyerr::<exc::IOError>(py).map(|s| PyUnicode::new(py, &s))
    }
});
