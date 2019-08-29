// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![allow(non_camel_case_types)]

use clidispatch::io::IO;
use cpython::*;
use cpython_ext::wrap_pyio;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "commands"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "run",
        py_fn!(
            py,
            run_py(
                args: Vec<String>,
                fin: PyObject,
                fout: PyObject,
                ferr: Option<PyObject> = None
            )
        ),
    )?;
    Ok(m)
}

fn run_py(
    _py: Python,
    args: Vec<String>,
    fin: PyObject,
    fout: PyObject,
    ferr: Option<PyObject>,
) -> PyResult<i32> {
    let fin = wrap_pyio(fin);
    let fout = wrap_pyio(fout);
    let ferr = ferr.map(wrap_pyio);

    let mut io = IO::new(fin, fout, ferr);
    Ok(hgcommands::run_command(args, &mut io))
}
