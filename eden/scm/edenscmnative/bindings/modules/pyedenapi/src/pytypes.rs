/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Python types that implements `ToPyObject`.

use cpython::*;
use edenapi::Stats;

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
