/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Support `ImplInto` from cpython-ext.

use cpython::*;
use cpython_ext::convert::register_into;

use crate::context;

pub(crate) fn register(py: Python) {
    register_into(py, |py, c: context| c.get_ctx(py));
}
