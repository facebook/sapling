/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Python types that implements `ToPyObject`.

use cpython::*;
use edenapi::Stats;
use edenapi_types::CommitRevlogData;

use crate::stats::stats;

/// Converts `Stats` to Python `stats`.
pub struct PyStats(pub Stats);

impl ToPyObject for PyStats {
    type ObjectType = stats;

    fn to_py_object(&self, py: Python) -> Self::ObjectType {
        stats::new(py, self.0.clone()).unwrap()
    }

    fn into_py_object(self, py: Python) -> Self::ObjectType {
        stats::new(py, self.0).unwrap()
    }
}

/// Converts `CommitRevlogData` to Python `(node: bytes, rawdata: bytes)`
pub struct PyCommitRevlogData(pub CommitRevlogData);

impl ToPyObject for PyCommitRevlogData {
    type ObjectType = PyObject;

    fn to_py_object(&self, py: Python) -> Self::ObjectType {
        let id = PyBytes::new(py, self.0.hgid.as_ref());
        let data = PyBytes::new(py, self.0.revlog_data.as_ref());
        (id, data).to_py_object(py).into_object()
    }
}
