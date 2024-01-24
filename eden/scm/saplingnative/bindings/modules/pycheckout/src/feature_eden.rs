/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use configmodel::Config;
use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::ResultPyErrExt;
use io::IO;
use parking_lot::RwLock;
use termlogger::TermLogger;
use workingcopy::workingcopy::WorkingCopy;

pub(crate) fn populate_module(py: Python, m: &PyModule) -> PyResult<()> {
    m.add(
        py,
        "edenredirectfixup",
        py_fn!(py, eden_redirect_fixup(config: ImplInto<Arc<dyn Config + Send + Sync>>, wc: ImplInto<Arc<RwLock<WorkingCopy>>>)),
    )?;
    Ok(())
}

fn eden_redirect_fixup(
    py: Python,
    config: ImplInto<Arc<dyn Config + Send + Sync>>,
    wc: ImplInto<Arc<RwLock<WorkingCopy>>>,
) -> PyResult<PyNone> {
    let io = IO::main().map_pyerr(py)?;
    let config = config.into();
    let wc = wc.into();
    checkout::edenfs::edenfs_redirect_fixup(&TermLogger::new(&io), &config, &wc.read())
        .map_pyerr(py)?;
    Ok(PyNone)
}
