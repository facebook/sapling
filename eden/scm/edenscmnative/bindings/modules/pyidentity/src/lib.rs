/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

extern crate identity as rsident;

use cpython::*;
use cpython_ext::error::Result;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::PyPathBuf;
use rsident::Identity;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "identity"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<identity>(py)?;
    m.add(py, "all", py_fn!(py, all()))?;
    m.add(py, "current", py_fn!(py, current()))?;
    m.add(py, "default", py_fn!(py, default()))?;
    m.add(py, "sniffenv", py_fn!(py, sniff_env()))?;
    m.add(py, "sniffroot", py_fn!(py, sniff_root(path: PyPathBuf)))?;
    m.add(py, "sniffdir", py_fn!(py, sniff_dir(path: PyPathBuf)))?;
    m.add(py, "envvar", py_fn!(py, try_env_var(suffix: PyString)))?;

    Ok(m)
}

py_class!(pub class identity |py| {
    data ident: Identity;

    def dotdir(&self) -> PyResult<String> {
        Ok(self.ident(py).dot_dir().to_string())
    }

    def __str__(&self) -> PyResult<String> {
        Ok(format!("{}", self.ident(py)))
    }

    def cliname(&self) -> PyResult<String> {
        Ok(self.ident(py).cli_name().to_string())
    }

    def configrepofile(&self) -> PyResult<String> {
        Ok(self.ident(py).config_repo_file().to_string())
    }

    def userconfigpaths(&self) -> PyResult<Vec<PyPathBuf>> {
        self.ident(py).user_config_paths().iter().map(|p| p.as_path().try_into()).collect::<Result<Vec<PyPathBuf>>>().map_pyerr(py)
    }

    def productname(&self) -> PyResult<String> {
        Ok(self.ident(py).product_name().to_string())
    }

    def longproductname(&self) -> PyResult<String> {
        Ok(self.ident(py).long_product_name().to_string())
    }
});

fn sniff_env(py: Python) -> PyResult<identity> {
    identity::create_instance(py, rsident::sniff_env())
}

fn sniff_root(py: Python, path: PyPathBuf) -> PyResult<Option<(PyPathBuf, identity)>> {
    Ok(match rsident::sniff_root(path.as_path()).map_pyerr(py)? {
        None => None,
        Some((path, ident)) => Some((
            path.try_into().map_pyerr(py)?,
            identity::create_instance(py, ident)?,
        )),
    })
}

fn sniff_dir(py: Python, path: PyPathBuf) -> PyResult<Option<identity>> {
    Ok(match rsident::sniff_dir(path.as_path()).map_pyerr(py)? {
        None => None,
        Some(ident) => Some(identity::create_instance(py, ident)?),
    })
}

fn try_env_var(py: Python, suffix: PyString) -> PyResult<Option<String>> {
    rsident::env_var(suffix.to_string(py)?.as_ref())
        .transpose()
        .map_pyerr(py)
}

fn current(py: Python) -> PyResult<identity> {
    identity::create_instance(py, rsident::IDENTITY.read().clone())
}

fn default(py: Python) -> PyResult<identity> {
    identity::create_instance(py, rsident::idents::DEFAULT.clone())
}

fn all(py: Python) -> PyResult<Vec<identity>> {
    rsident::idents::ALL_IDENTITIES
        .iter()
        .map(|id| identity::create_instance(py, id.clone()))
        .collect()
}
