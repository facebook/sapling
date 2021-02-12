/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use anyhow::{anyhow, Context};
use cpython::*;
use url::Url;

use auth::{self, AuthConfig};
use cpython_ext::{PyNone, PyPath, ResultPyErrExt};
use pyconfigparser::config;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "auth"].join(".");
    let m = PyModule::new(py, &name)?;

    m.add(
        py,
        "getauth",
        py_fn!(
            py,
            getauth(
                cfg: config,
                uri: &str,
                user: Option<&str> = None,
                validate: bool = true
            )
        ),
    )?;
    m.add(py, "checkcert", py_fn!(py, checkcert(cert: &PyPath)))?;

    Ok(m)
}

fn getauth(
    py: Python,
    cfg: config,
    uri: &str,
    user: Option<&str>,
    validate: bool,
) -> PyResult<PyObject> {
    let cfg = &cfg.get_cfg(py);
    let mut uri = uri
        .parse::<Url>()
        .context("failed to parse URL")
        .map_pyerr(py)?;

    if let Some(user) = user {
        uri.set_username(user)
            .map_err(|()| anyhow!("failed to set username in URL"))
            .map_pyerr(py)?;
    }

    AuthConfig::new(&cfg)
        .validate(validate)
        .auth_for_url(&uri)
        .map_pyerr(py)?
        .map_or_else(
            || Ok(PyNone.to_py_object(py).into_object()),
            |auth| {
                let cert = auth.cert.as_ref().map(|path| path.to_string_lossy());
                let key = auth.key.as_ref().map(|path| path.to_string_lossy());
                let cacerts = auth.cacerts.as_ref().map(|path| path.to_string_lossy());

                let dict = PyDict::new(py);

                dict.set_item(py, "cert", cert)?;
                dict.set_item(py, "key", key)?;
                dict.set_item(py, "cacerts", cacerts)?;
                dict.set_item(py, "prefix", &auth.prefix)?;
                dict.set_item(py, "username", &auth.username)?;
                dict.set_item(py, "schemes", &auth.schemes)?;
                dict.set_item(py, "priority", &auth.priority)?;

                for (k, v) in &auth.extras {
                    dict.set_item(py, k, v)?;
                }

                Ok((&auth.group, dict).to_py_object(py).into_object())
            },
        )
}

fn checkcert(py: Python, cert: &PyPath) -> PyResult<PyNone> {
    auth::check_certs(cert).map_pyerr(py).map(|_| PyNone)
}
