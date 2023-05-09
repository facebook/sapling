/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Support `ImplInto` from cpython-ext.

use cpython::*;
use cpython_ext::convert::register_into;

use crate::config;

pub(crate) fn register(py: Python) {
    register_into(py, |py, c: config| c.get_config_trait(py));
    register_into(py, |py, c: config| c.get_thread_safe_config_trait(py));
}
