/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use commands::prepare_builtin_modules;
use cpython::py_module_initializer;

py_module_initializer!(bindings, initbindings, PyInit_bindings, |py, m| {
    m.add(py, "__doc__", "Bootstraps the hg python environment")?;
    commands::init();
    prepare_builtin_modules(py, m)?;
    Ok(())
});
