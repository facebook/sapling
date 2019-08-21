// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::configparser::config;
use clidispatch::global_flags::HgGlobalOpts;
use cliparser::alias::{expand_aliases, expand_prefix};
use cliparser::parser::*;
use cpython::*;
use cpython_ext::Bytes;
use std::collections::{BTreeMap, HashMap};

mod exceptions {
    use super::*;

    py_exception!(cliparser, AmbiguousCommand);
    py_exception!(cliparser, CircularReference);
    py_exception!(cliparser, MalformedAlias);
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
        m.add(py, "MalformedAlias", MalformedAlias::type_object(py))?;
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
    let mut flags: Vec<Flag> = definitions
        .into_iter()
        .map(|(c, s, v)| (c.chars().nth(0), s, "", v).into())
        .collect();
    flags.extend(HgGlobalOpts::flags());

    let result = ParseOptions::new()
        .flag_alias("repo", "repository")
        .flags(flags)
        .error_on_unknown_opts(true)
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
    let cfg = &config.get_cfg(py);

    if !strict && !args.is_empty() {
        // Expand args[0] from a prefix to a full command name
        let mut command_map = BTreeMap::new();
        for (i, name) in command_names.into_iter().enumerate() {
            let i: isize = i as isize + 1; // avoid using 0
            let multiples = expand_command_name(name);
            let is_debug = multiples.iter().any(|s| s.starts_with("debug"));
            for multiple in multiples.into_iter() {
                command_map.insert(multiple, if is_debug { -i } else { i });
            }
        }

        // Add command names from the alias configuration.
        // XXX: This duplicates with clidispatch. They should be de-duplicated.
        for name in cfg.keys("alias") {
            if let Ok(name) = String::from_utf8(name.to_vec()) {
                let is_debug = name.starts_with("debug");
                let i = command_map.len() as isize;
                command_map.insert(name, if is_debug { -i } else { i });
            }
        }

        args[0] =
            expand_prefix(&command_map, args[0].clone()).map_err(|e| map_to_python_err(py, e))?;
    }

    let lookup = move |name: &str| match (cfg.get("alias", name), cfg.get("defaults", name)) {
        (None, None) => None,
        (Some(v), None) => String::from_utf8(v.to_vec()).ok(),
        (None, Some(v)) => String::from_utf8(v.to_vec())
            .ok()
            .map(|v| format!("{} {}", name, v)),
        (Some(a), Some(d)) => {
            if let (Ok(a), Ok(d)) = (String::from_utf8(a.to_vec()), String::from_utf8(d.to_vec())) {
                // XXX: This makes defaults override alias if there are conflicted
                // flags. The desired behavior is to make alias override defaults.
                // However, [defaults] is deprecated and is likely only used
                // by tests. So this might be fine.
                Some(format!("{} {}", a, d))
            } else {
                None
            }
        }
    };

    let (expanded_args, replaced_aliases) =
        expand_aliases(lookup, &args).map_err(|e| map_to_python_err(py, e))?;

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
    let result = ParseOptions::new()
        .ignore_prefix(true)
        .early_parse(true)
        .flag_alias("repo", "repository")
        .flags(HgGlobalOpts::flags())
        .parse_args(&args)
        .map_err(|e| map_to_python_err(py, e))?;
    let rust_opts = result.opts().clone();
    let mut opts = HashMap::new();

    for (key, value) in rust_opts {
        let val: PyObject = value.into_py_object(py).into_object();
        opts.insert(key, val);
    }
    Ok(opts)
}

fn parse_args(py: Python, args: Vec<String>) -> PyResult<Vec<String>> {
    let result = ParseOptions::new()
        .flag_alias("repo", "repository")
        .flags(HgGlobalOpts::flags())
        .parse_args(&args)
        .map_err(|e| map_to_python_err(py, e))?;
    let arguments = result.args().clone();

    Ok(arguments)
}

fn parse(
    py: Python,
    args: Vec<String>,
    keep_sep: bool,
) -> PyResult<(Vec<Bytes>, HashMap<Bytes, PyObject>, usize)> {
    let result = ParseOptions::new()
        .flag_alias("repo", "repository")
        .flags(HgGlobalOpts::flags())
        .keep_sep(keep_sep)
        .parse_args(&args)
        .map_err(|e| map_to_python_err(py, e))?;

    let arguments = result.args().iter().cloned().map(Bytes::from).collect();
    let opts = result
        .opts()
        .iter()
        .map(|(k, v)| (Bytes::from(k.clone()), v.to_py_object(py).into_object()))
        .collect();

    Ok((arguments, opts, result.first_arg_index()))
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
        ParseError::MalformedAlias { name, value } => {
            return PyErr::new::<exceptions::MalformedAlias, _>(py, (msg, name, value));
        }
    }
}
