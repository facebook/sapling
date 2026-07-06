/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use ::context::CoreContext;
use clidispatch::acl;
use configset::ConfigSet;
use cpython::*;
use cpython_ext::convert::ImplInto;
use cpython_ext::error::ResultPyErrExt;
use io::IO;

mod impl_into;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "context"].join(".");
    let m = PyModule::new(py, &name)?;

    m.add_class::<context>(py)?;
    m.add(py, "check_permission_denied", py_fn!(py, check_permission_denied(ctx: ImplInto<CoreContext>) -> PyResult<(Option<String>, Vec<String>, bool)> {
        let ctx: CoreContext = ctx.into();
        let result = acl::check_permission_denied_paths(&ctx.permission_denied_paths, &ctx.config).map_pyerr(py)?;
        Ok((result.warning_message, result.acl_details, result.exit_nonzero))
    }))?;
    m.add(
        py,
        "format_permission_denied",
        py_fn!(py, format_permission_denied(
            ctx: ImplInto<CoreContext>,
            path: String,
            hgid: String,
            request_acl: String
        ) -> PyResult<String> {
            let ctx: CoreContext = ctx.into();
            let err = types::errors::PermissionDenied {
                path: path.try_into().map_pyerr(py)?,
                hgid: hgid.parse().map_pyerr(py)?,
                request_acl,
            };
            Ok(acl::format_permission_denied_error(&err, ctx.config.as_ref()))
        }),
    )?;

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
        let mut new_ctx = CoreContext::new(
            Arc::new(config.get_cfg(py)),
            ctx.io.clone(),
            ctx.raw_args.clone(),
        );
        new_ctx.permission_denied_paths = ctx.permission_denied_paths.clone();
        Self::create_instance(py, new_ctx)
    }

    def config(&self) -> PyResult<pyconfigloader::config> {
        pyconfigloader::config::create_instance(py, RefCell::new(
            ConfigSet::wrap(self.ctx(py).config.clone())
        ))
    }

    def permission_denied_paths(&self) -> PyResult<Vec<String>> {
        let ctx = self.ctx(py);
        let paths = ctx.permission_denied_paths.lock();
        let mut seen = HashSet::new();
        Ok(paths
            .iter()
            .filter_map(|err| {
                let s = err.path.to_string();
                if seen.insert(s.clone()) { Some(s) } else { None }
            })
            .collect())
    }

    def permission_denied_count(&self) -> PyResult<u64> {
        let ctx = self.ctx(py);
        Ok(ctx.permission_denied_paths.count())
    }

});

impl context {
    pub fn get_ctx(&self, py: Python) -> CoreContext {
        self.ctx(py).clone()
    }
}
