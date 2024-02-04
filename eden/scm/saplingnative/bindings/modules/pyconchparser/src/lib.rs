/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// conch_parser is separate from saplingnative/bindings intentionally
// so it can be used standalone without coupling with the rest of
// sapling logic.

use conch_parser::lexer::Lexer;
use conch_parser::parse::DefaultParser;
use cpython::*;
use cpython_ext::ser::to_object;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "conchparser"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "parse", py_fn!(py, parse(code: &str)))?;
    Ok(m)
}

fn parse(py: Python, code: &str) -> PyResult<PyObject> {
    let lex = Lexer::new(code.chars());
    let mut parser = DefaultParser::new(lex);
    let mut commands = Vec::new();
    while let Some(command) = parser
        .complete_command()
        .map_err(|e| PyErr::new::<exc::ValueError, _>(py, e.to_string()))?
    {
        commands.push(command)
    }
    to_object(py, &commands)
}
