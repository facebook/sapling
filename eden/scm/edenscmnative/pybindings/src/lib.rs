/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::py_module_initializer;
use hgcommands::prepare_builtin_modules;

py_module_initializer!(bindings, initbindings, PyInit_bindings, |py, m| {
    m.add(py, "__doc__", "Bootstraps the hg python environment")?;
    prepare_builtin_modules(py, m)?;
    Ok(())
});
