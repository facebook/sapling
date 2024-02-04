/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// conch_parser is separate from saplingnative/bindings intentionally
// so it can be used standalone without coupling with the rest of
// sapling logic.

use cpython::serde::to_py_object;
use cpython::*;
use third_party_conch_parser::lexer::Lexer;
use third_party_conch_parser::parse::DefaultParser;

// rustfmt turns "|py, m|" into "|py, m,|" and breaks compile
#[rustfmt::skip::macros(py_module_initializer)]
py_module_initializer!(conch_parser, initconch_parser, PyInit_conch_parser, |py, m| {
    m.add(py, "parse", py_fn!(py, parse(code: &str)))?;
    Ok(())
});

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
    to_py_object(py, &commands)
}
