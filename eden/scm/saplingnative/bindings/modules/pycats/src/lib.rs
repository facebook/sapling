/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use cats::CatTokenType;
use cats::CatsSection;
use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::ResultPyErrExt;
use pyconfigloader::config;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "cats"].join(".");
    let m = PyModule::new(py, &name)?;

    m.add(
        py,
        "find_cats_by_type",
        py_fn!(
            py,
            find_cats_by_type(
                cfg: config,
                section_name: &str,
                token_type: &str,
                raise_if_missing: bool = true
            )
        ),
    )?;

    m.add(
        py,
        "get_cats_by_type",
        py_fn!(
            py,
            get_cats_by_type(
                cfg: config,
                section_name: &str,
                token_type: &str,
                raise_if_missing: bool = true
            )
        ),
    )?;

    Ok(m)
}

fn parse_token_type(py: Python, token_type: &str) -> PyResult<CatTokenType> {
    CatTokenType::from_type_str(token_type).map_pyerr(py)
}

fn find_cats_by_type(
    py: Python,
    cfg: config,
    section_name: &str,
    token_type: &str,
    raise_if_missing: bool,
) -> PyResult<PyObject> {
    let cfg = &cfg.get_cfg(py);
    let token_type = parse_token_type(py, token_type)?;

    CatsSection::from_config(cfg, section_name)
        .find_cats_by_type(token_type)
        .or_else(|e| if raise_if_missing { Err(e) } else { Ok(None) })
        .map_pyerr(py)?
        .map_or_else(
            || Ok(PyNone.to_py_object(py).into_object()),
            |group| {
                let dict = PyDict::new(py);

                if let Some(path) = group.path {
                    dict.set_item(py, "path", path.to_string_lossy())?;
                }

                if group.priority > 0 {
                    dict.set_item(py, "priority", group.priority)?;
                }

                let type_str = match group.token_type {
                    CatTokenType::Forwarded => "forwarded",
                    CatTokenType::Auth => "auth",
                };
                dict.set_item(py, "type", type_str)?;

                Ok((&group.name, dict).to_py_object(py).into_object())
            },
        )
}

fn get_cats_by_type(
    py: Python,
    cfg: config,
    section_name: &str,
    token_type: &str,
    raise_if_missing: bool,
) -> PyResult<PyObject> {
    let cfg = &cfg.get_cfg(py);
    let token_type = parse_token_type(py, token_type)?;

    CatsSection::from_config(cfg, section_name)
        .get_cats_by_type(token_type)
        .or_else(|e| if raise_if_missing { Err(e) } else { Ok(None) })
        .map_pyerr(py)?
        .map_or_else(
            || Ok(PyNone.to_py_object(py).into_object()),
            |cats_content| Ok(cats_content.to_py_object(py).into_object()),
        )
}
