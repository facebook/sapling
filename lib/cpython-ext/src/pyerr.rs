// Copyright Facebook, Inc. 2019
use cpython::*;

pub fn format_py_error(py: Python, err: &PyErr) -> PyResult<String> {
    let traceback = PyModule::import(py, "traceback")?;
    let py_message = traceback.call(
        py,
        "format_exception",
        (&err.ptype, &err.pvalue, &err.ptraceback),
        None,
    )?;

    let py_lines = PyList::extract(py, &py_message)?;

    let lines: Vec<String> = py_lines
        .iter(py)
        .map(|l| l.extract::<String>(py).unwrap_or_default())
        .collect();

    Ok(lines.join(""))
}
