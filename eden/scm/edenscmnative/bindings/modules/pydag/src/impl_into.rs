/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Support `ImplInto` from cpython-ext.

use cpython::*;
use cpython_ext::convert::register_into;

use crate::commits::commits;
use crate::nameset::nameset;

pub(crate) fn register(py: Python) {
    register_into(py, |py, c: commits| c.to_read_root_tree_nodes(py));
    register_into(py, |py, s: nameset| s.to_native_set(py));
}
