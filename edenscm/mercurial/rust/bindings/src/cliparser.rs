// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::configparser::config;
use cliparser::alias::{expand_aliases, Error};
use cliparser::hgflags::global_hg_flag_definitions;
use cliparser::parser::*;
use cpython::*;
use std::collections::{BTreeMap, HashMap};

mod exceptions {
    use super::*;

    py_exception!(cliparser, AmbiguousCommand);
    py_exception!(cliparser, CircularReference);

}

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "cliparser"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "earlyparse", py_fn!(py, early_parse(args: Vec<String>)))?;
    m.add(py, "parseargs", py_fn!(py, parse_args(args: Vec<String>)))?;
    m.add(
        py,
        "parse",
        py_fn!(py, parse(args: Vec<String>, keep_sep: bool)),
    )?;
    m.add(
        py,
        "expandargs",
        py_fn!(
            py,
            expand_args(
                config: config,
                command_names: Vec<String>,
                arg: String,
                strict: bool = false
            )
        ),
    )?;
    {
        use exceptions::*;
        m.add(py, "AmbiguousCommand", AmbiguousCommand::type_object(py))?;
        m.add(py, "CircularReference", CircularReference::type_object(py))?;
    }
    Ok(m)
}

fn expand_args(
    py: Python,
    config: config,
    command_names: Vec<String>,
    arg: String,
    strict: bool,
) -> PyResult<(Vec<PyBytes>, Vec<PyBytes>)> {
    let mut alias_map = BTreeMap::new();
    let cfg = &config.get_cfg(py);
    let alias_keys = cfg.keys("alias");

    for alias_key in alias_keys {
        let key = String::from_utf8(alias_key.to_vec()).unwrap();
        let alias_val = cfg.get("alias", alias_key).unwrap();
        let val = String::from_utf8(alias_val.to_vec()).unwrap();
        alias_map.insert(key, val);
    }

    let mut command_map = BTreeMap::new();
    for (i, name) in command_names.into_iter().enumerate() {
        let multiples = expand_command_name(name);
        for multiple in multiples.into_iter() {
            command_map.insert(multiple, i);
        }
    }

    let (expanded_args, replaced_aliases) = expand_aliases(&alias_map, &command_map, arg, strict)
        .map_err(|e| {
        let msg = format!("{}", e);
        match e {
            Error::AmbiguousCommand {
                command_name,
                possibilities,
            } => {
                return PyErr::new::<exceptions::AmbiguousCommand, _>(
                    py,
                    (msg, command_name, possibilities),
                )
            }
            Error::CircularReference { command_name } => {
                return PyErr::new::<exceptions::CircularReference, _>(py, (msg, command_name))
            }
        }
    })?;

    let expanded_args: Vec<PyBytes> = expanded_args
        .into_iter()
        .map(|v| PyBytes::new(py, &(v.as_ref())))
        .collect();

    let replaced_aliases: Vec<PyBytes> = replaced_aliases
        .into_iter()
        .map(|v| PyBytes::new(py, &(v.as_ref())))
        .collect();

    Ok((expanded_args, replaced_aliases))
}

fn expand_command_name(name: String) -> Vec<String> {
    name.trim_start_matches("^")
        .split("|")
        .map(|s| s.to_string())
        .collect()
}

fn early_parse(py: Python, args: Vec<String>) -> PyResult<HashMap<String, PyObject>> {
    let parsing_options = OpenOptions::new()
        .ignore_prefix(true)
        .early_parse(true)
        .flag_alias("repo", "repository");
    let definitions = global_hg_flag_definitions();
    let flags = Flag::from_flags(&definitions);
    let parser = Parser::new(&flags).with_parsing_options(parsing_options);
    let result = parser.parse_args(&args).unwrap();
    let rust_opts = result.opts().clone();
    let mut opts = HashMap::new();

    for (key, value) in rust_opts {
        let val: PyObject = match value {
            Value::Bool(b) => b.to_py_object(py).into_object(),
            Value::Str(s) => s.to_py_object(py).into_object(),
            Value::Int(i) => i.to_py_object(py).into_object(),
            Value::List(vec) => vec.to_py_object(py).into_object(),
        };
        opts.insert(key, val);
    }
    Ok(opts)
}

fn parse_args(_py: Python, args: Vec<String>) -> PyResult<Vec<String>> {
    let parsing_options = OpenOptions::new().flag_alias("repo", "repository");
    let definitions = global_hg_flag_definitions();
    let flags = Flag::from_flags(&definitions);
    let parser = Parser::new(&flags).with_parsing_options(parsing_options);
    let result = parser.parse_args(&args).unwrap();
    let arguments = result.args().clone();

    Ok(arguments)
}

fn parse(py: Python, args: Vec<String>, keep_sep: bool) -> PyResult<PyTuple> {
    let parsing_options = OpenOptions::new()
        .flag_alias("repo", "repository")
        .keep_sep(keep_sep);
    let definitions = global_hg_flag_definitions();
    let flags = Flag::from_flags(&definitions);
    let parser = Parser::new(&flags).with_parsing_options(parsing_options);
    let result = parser.parse_args(&args).unwrap();

    let arguments = result.args().to_py_object(py).into_object();
    let rust_opts = result.opts();
    let mut opts = HashMap::new();

    for (key, value) in rust_opts {
        let val: PyObject = match value {
            Value::Bool(b) => b.to_py_object(py).into_object(),
            Value::Str(s) => s.to_py_object(py).into_object(),
            Value::Int(i) => i.to_py_object(py).into_object(),
            Value::List(vec) => vec.to_py_object(py).into_object(),
        };
        opts.insert(key, val);
    }

    Ok(PyTuple::new(
        py,
        &[arguments, opts.to_py_object(py).into_object()],
    ))
}
