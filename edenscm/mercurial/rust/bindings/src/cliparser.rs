// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::configparser::config;
use cliparser::alias::{expand_aliases, expand_prefix};
use cliparser::hgflags::global_hg_flag_definitions;
use cliparser::parser::*;
use cpython::*;
use cpython_ext::Bytes;
use std::collections::{BTreeMap, HashMap};

mod exceptions {
    use super::*;

    py_exception!(cliparser, AmbiguousCommand);
    py_exception!(cliparser, CircularReference);
    py_exception!(cliparser, IllformedAlias);
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
                args: Vec<String>,
                strict: bool = false
            )
        ),
    )?;
    m.add(
        py,
        "parsecommand",
        py_fn!(
            py,
            parse_command(args: Vec<String>, definitions: Vec<(String, String, Value)>)
        ),
    )?;
    {
        use exceptions::*;
        m.add(py, "AmbiguousCommand", AmbiguousCommand::type_object(py))?;
        m.add(py, "CircularReference", CircularReference::type_object(py))?;
        m.add(py, "IllformedAlias", IllformedAlias::type_object(py))?;
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

fn parse_command(
    py: Python,
    args: Vec<String>,
    definitions: Vec<(String, String, Value)>,
) -> PyResult<(Vec<PyBytes>, HashMap<Bytes, Value>)> {
    let rust_definitions: Vec<FlagDefinition> = definitions
        .into_iter()
        .map(|(c, s, v)| (c.chars().next().unwrap_or(' '), s.into(), "".into(), v))
        .collect();

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

    let opts = result.opts().clone();

    let options: HashMap<Bytes, Value> = opts.into_iter().map(|(k, v)| (k.into(), v)).collect();

    Ok((arguments, options))
}
fn expand_args(
    py: Python,
    config: config,
    command_names: Vec<String>,
    mut args: Vec<String>,
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

    if !strict && !args.is_empty() {
        // Expand args[0] from a prefix to a full command name
        let mut command_map = BTreeMap::new();
        for (i, name) in command_names.into_iter().enumerate() {
            let multiples = expand_command_name(name);
            for multiple in multiples.into_iter() {
                command_map.insert(multiple, i);
            }
        }
        args[0] =
            expand_prefix(&command_map, args[0].clone()).map_err(|e| map_to_python_err(py, e))?;
    }

    let (expanded_args, replaced_aliases) =
        expand_aliases(&alias_map, &args).map_err(|e| map_to_python_err(py, e))?;

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
        let val: PyObject = value.into_py_object(py).into_object();
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
        let val: PyObject = value.to_py_object(py).into_object();
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
        ParseError::IllformedAlias { name, value } => {
            return PyErr::new::<exceptions::IllformedAlias, _>(py, (msg, name, value));
        }
    }
}
