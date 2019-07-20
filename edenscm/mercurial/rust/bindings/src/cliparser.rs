// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cliparser::hgflags::global_hg_flag_definitions;
use cliparser::parser::*;
use cpython::*;

use std::collections::HashMap;

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
    Ok(m)
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
