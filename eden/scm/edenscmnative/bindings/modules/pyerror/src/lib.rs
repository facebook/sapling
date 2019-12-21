/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::failure as error;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "error"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "IndexedLogError",
        py.get_type::<error::PyIndexedLogError>(),
    )?;
    Ok(m)
}
