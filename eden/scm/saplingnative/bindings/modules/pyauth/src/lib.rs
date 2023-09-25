/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use anyhow::anyhow;
use anyhow::Context;
use auth::AuthSection;
use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::ResultPyErrExt;
use pyconfigloader::config;
use url::Url;

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
                raise_if_missing: bool = true
            )
        ),
    )?;

    Ok(m)
}

fn getauth(
    py: Python,
    cfg: config,
    uri: &str,
    user: Option<&str>,
    raise_if_missing: bool,
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

    AuthSection::from_config(cfg)
        .best_match_for(&uri)
        .or_else(|e| if raise_if_missing { Err(e) } else { Ok(None) })
        .map_pyerr(py)?
        .map_or_else(
            || Ok(PyNone.to_py_object(py).into_object()),
            |group| {
                let dict = PyDict::new(py);

                dict.set_item(py, "prefix", &group.prefix)?;
                dict.set_item(py, "schemes", group.schemes.join(" "))?;

                if let Some(cert) = group.cert {
                    dict.set_item(py, "cert", cert.to_string_lossy())?;
                }

                if let Some(key) = group.key {
                    dict.set_item(py, "key", key.to_string_lossy())?;
                }

                if let Some(cacerts) = group.cacerts {
                    dict.set_item(py, "cacerts", cacerts.to_string_lossy())?;
                }

                if let Some(username) = group.username {
                    dict.set_item(py, "username", username)?;
                }

                if group.priority > 0 {
                    dict.set_item(py, "priority", group.priority)?;
                }

                for (k, v) in &group.extras {
                    dict.set_item(py, k, v)?;
                }

                Ok((&group.name, dict).to_py_object(py).into_object())
            },
        )
}
