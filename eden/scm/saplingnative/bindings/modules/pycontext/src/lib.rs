/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::sync::Arc;

use ::context::CoreContext;
use configset::ConfigSet;
use cpython::*;
use cpython_ext::error::ResultPyErrExt;
use io::IO;

mod impl_into;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "context"].join(".");
    let m = PyModule::new(py, &name)?;

    m.add_class::<context>(py)?;

    impl_into::register(py);

    Ok(m)
}

py_class!(pub class context |py| {
    data ctx: CoreContext;

    def __new__(_cls) -> PyResult<Self> {
        Self::create_instance(py, CoreContext::new(
            Arc::new(ConfigSet::new().named("pycontext")),
            IO::main().map_pyerr(py)?,
            vec![],
        ))
    }

    def withconfig(&self, config: &pyconfigloader::config) -> PyResult<Self> {
        let ctx = self.ctx(py);
        Self::create_instance(py, CoreContext::new(
            Arc::new(config.get_cfg(py)),
            ctx.io.clone(),
            ctx.raw_args.clone(),
        ))
    }

    def config(&self) -> PyResult<pyconfigloader::config> {
        pyconfigloader::config::create_instance(py, RefCell::new(ConfigSet::wrap(self.ctx(py).config.clone())))
    }
});

impl context {
    pub fn get_ctx(&self, py: Python) -> CoreContext {
        self.ctx(py).clone()
    }
}
