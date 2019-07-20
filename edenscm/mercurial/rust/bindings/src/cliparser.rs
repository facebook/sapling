// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::configparser::config;
use cliparser::alias::expand_aliases;
use cliparser::hgflags::global_hg_flag_definitions;
use cliparser::parser::*;
use cpython::*;
use std::collections::{BTreeMap, HashMap};

mod exceptions {
    use super::*;

    py_exception!(cliparser, AmbiguousCommand);
    py_exception!(cliparser, CircularReference);
    py_exception!(cliparser, OptionNotRecognized);
    py_exception!(cliparser, OptionRequiresArgument);
    py_exception!(cliparser, OptionArgumentInvalid);
    py_exception!(cliparser, OptionAmbiguous);

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
    m.add(
        py,
        "parsecommand",
        py_fn!(
            py,
            parse_command(args: Vec<String>, definitions: Vec<PyTuple>)
        ),
    )?;
    {
        use exceptions::*;
        m.add(py, "AmbiguousCommand", AmbiguousCommand::type_object(py))?;
        m.add(py, "CircularReference", CircularReference::type_object(py))?;
        m.add(
            py,
            "OptionNotRecognized",
            OptionNotRecognized::type_object(py),
        )?;
        m.add(
            py,
            "OptionRequiresArgument",
            OptionRequiresArgument::type_object(py),
        )?;
        m.add(
            py,
            "OptionArgumentInvalid",
            OptionArgumentInvalid::type_object(py),
        )?;
        m.add(py, "OptionAmbiguous", OptionAmbiguous::type_object(py))?;
    }
    Ok(m)
}

fn parse_command(py: Python, args: Vec<String>, definitions: Vec<PyTuple>) -> PyResult<PyTuple> {
    let mut rust_definitions: Vec<FlagDefinition> = Vec::new();
    for definition in definitions {
        let short_obj = definition.get_item(py, 0);
        let long_obj = definition.get_item(py, 1);
        let val_obj = definition.get_item(py, 2);
        let short = convert_short(py, short_obj);
        let long = convert_long(py, long_obj);
        let val = convert_val(py, val_obj);
        rust_definitions.push((short, long.into(), "".into(), val));
    }

    let parsing_options = OpenOptions::new()
        .flag_alias("repo", "repository")
        .error_on_unknown_opts(true);
    let mut flags = Flag::from_flags(&rust_definitions);
    let global_defs = global_hg_flag_definitions();
    let globals = Flag::from_flags(&global_defs);
    flags.extend(globals);
    let parser = Parser::new(&flags).with_parsing_options(parsing_options);
    let result = parser
        .parse_args(&args)
        .map_err(|e| map_to_python_err(py, e))?;

    let arguments: Vec<PyBytes> = result
        .args()
        .clone()
        .into_iter()
        .map(|s| PyBytes::new(py, s.as_bytes()))
        .collect();

    let rust_opts = result.opts();
    let mut opts = HashMap::new();

    for (key, value) in rust_opts {
        let val: PyObject = match value {
            Value::OptBool() => py.None().into_object(),
            Value::Bool(b) => b.to_py_object(py).into_object(),
            Value::Str(s) => PyBytes::new(py, s.as_bytes()).into_object(),
            Value::Int(i) => i.to_py_object(py).into_object(),
            Value::List(vec) => {
                let converted: Vec<PyBytes> = vec
                    .into_iter()
                    .map(|s| PyBytes::new(py, s.as_bytes()))
                    .collect();
                converted.to_py_object(py).into_object()
            }
        };
        opts.insert(key, val);
    }

    Ok(PyTuple::new(
        py,
        &[
            arguments.to_py_object(py).into_object(),
            opts.to_py_object(py).into_object(),
        ],
    ))
}

fn convert_short(py: Python, short: PyObject) -> char {
    let short_str = convert_long(py, short);
    short_str.chars().next().unwrap_or(' ')
}

fn convert_long(py: Python, long: PyObject) -> String {
    let py_str = long.cast_into::<PyString>(py).unwrap();
    return py_str.to_string(py).unwrap().to_string();
}

fn convert_val(py: Python, val: PyObject) -> Value {
    if let Ok(b) = val.cast_as::<PyBool>(py) {
        return Value::Bool(b.is_true());
    }

    if let Ok(_l) = val.cast_as::<PyList>(py) {
        return Value::List(Vec::new());
    }

    if let Ok(s) = val.cast_as::<PyString>(py) {
        return Value::Str(s.to_string(py).unwrap().to_string());
    }

    if let Ok(_i) = val.cast_as::<PyInt>(py) {
        return Value::Int(val.extract::<i64>(py).unwrap());
    }

    Value::OptBool()
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
        .map_err(|e| map_to_python_err(py, e))?;

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
            Value::OptBool() => py.None().into_object(),
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

    let arguments = result.args().clone().to_py_object(py).into_object();
    let rust_opts = result.opts().clone();
    let mut opts = HashMap::new();

    for (key, value) in rust_opts {
        let val: PyObject = match value {
            Value::OptBool() => py.None().into_object(),
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

fn map_to_python_err(py: Python, err: ParseError) -> PyErr {
    let msg = format!("{}", err);
    match err {
        ParseError::OptionNotRecognized { option_name } => {
            return PyErr::new::<exceptions::OptionNotRecognized, _>(py, (msg, option_name));
        }
        ParseError::OptionRequiresArgument { option_name } => {
            return PyErr::new::<exceptions::OptionRequiresArgument, _>(py, (msg, option_name));
        }
        ParseError::OptionArgumentInvalid {
            option_name,
            given,
            expected,
        } => {
            return PyErr::new::<exceptions::OptionArgumentInvalid, _>(
                py,
                (msg, option_name, given, expected),
            );
        }
        ParseError::OptionAmbiguous {
            option_name,
            possibilities,
        } => {
            return PyErr::new::<exceptions::OptionAmbiguous, _>(
                py,
                (msg, option_name, possibilities),
            )
        }
        ParseError::AmbiguousCommand {
            command_name,
            possibilities,
        } => {
            return PyErr::new::<exceptions::AmbiguousCommand, _>(
                py,
                (msg, command_name, possibilities),
            )
        }
        ParseError::CircularReference { command_name } => {
            return PyErr::new::<exceptions::CircularReference, _>(py, (msg, command_name))
        }
    }
}
