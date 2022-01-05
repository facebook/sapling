/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// This module contains functions from the Mercurial's diffhelpers.

use cpython::*;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "diffhelpers"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "addlines",
        py_fn!(
            py,
            addlines(
                fp: PyObject,
                hunk: PyList,
                lena: usize,
                lenb: usize,
                a: PyList,
                b: PyList
            )
        ),
    )?;
    m.add(
        py,
        "fix_newline",
        py_fn!(py, fix_newline(hunk: &PyList, a: &PyList, b: &PyList)),
    )?;
    m.add(
        py,
        "testhunk",
        py_fn!(py, testhunk(a: PyList, b: PyList, bstart: usize)),
    )?;
    Ok(m)
}

// Read lines from fp into the hunk.  The hunk is parsed into two arrays
// a and b. a gets the old state of the text, b gets the new state
// The control char from the hunk is saved when inserting into a, but not b
// (for performance while deleting files)
fn addlines(
    py: Python,
    fp: PyObject,
    hunk: PyList,
    lena: usize,
    lenb: usize,
    a: PyList,
    b: PyList,
) -> PyResult<usize> {
    let mut fp_iter = fp.iter(py)?;
    loop {
        let todoa = lena - a.len(py);
        let todob = lenb - b.len(py);
        let num = todoa.max(todob);
        if num == 0 {
            break;
        }
        for _i in 0..num {
            let s: PyBytes = match fp_iter.next() {
                Some(s) => s?.extract(py)?,
                None => {
                    return Err(PyErr::new::<exc::IOError, _>(
                        py,
                        "hunk processing error - hunk too short",
                    ));
                }
            };
            let s = s.data(py);
            if s.starts_with(b"\\ No newline at end of file") {
                fix_newline(py, &hunk, &a, &b)?;
                continue;
            }
            // Some patches may be missing the control char
            // on empty lines. Supply a leading space.
            let s = if s == b"\n" { b" \n" } else { s };
            hunk.append(py, to_object(py, s));
            match s.get(0) {
                Some(b'+') => b.append(py, to_object(py, &s[1..])),
                Some(b'-') => a.append(py, to_object(py, s)),
                _ => {
                    a.append(py, to_object(py, s));
                    if s.is_empty() {
                        // Ignore empty lines at EOF
                    } else {
                        b.append(py, to_object(py, &s[1..]));
                    }
                }
            }
        }
    }
    Ok(0)
}

// Fixup the last lines of a and b when the patch has no newline at EOF.
fn fix_newline(py: Python, hunk: &PyList, a: &PyList, b: &PyList) -> PyResult<usize> {
    let hunk_len = hunk.len(py);
    if hunk_len > 0 {
        let last_line = hunk.get_item(py, hunk_len - 1).extract::<PyBytes>(py)?;
        let last_line = last_line.data(py);
        let last_line = if last_line.ends_with(b"\r\n") {
            &last_line[..last_line.len() - 2]
        } else if last_line.ends_with(b"\n") {
            &last_line[..last_line.len() - 1]
        } else {
            last_line
        };
        match last_line.get(0) {
            Some(b' ') => {
                b.set_item(py, b.len(py) - 1, to_object(py, &last_line[1..]));
                a.set_item(py, a.len(py) - 1, to_object(py, &last_line[..]));
            }
            Some(b'+') => {
                b.set_item(py, b.len(py) - 1, to_object(py, &last_line[1..]));
            }
            Some(b'-') => {
                a.set_item(py, a.len(py) - 1, to_object(py, &last_line[..]));
            }
            _ => {}
        }
        hunk.set_item(py, hunk_len - 1, to_object(py, &last_line));
    }
    Ok(0)
}

// Compare the lines in a with the lines in b.  a is assumed to have
// a control char at the start of each line, this char is ignored in the
// compare
fn testhunk(py: Python, a: PyList, b: PyList, bstart: usize) -> PyResult<isize> {
    let alen = a.len(py);
    let blen = b.len(py);
    if alen + bstart > blen {
        return Ok(-1);
    }
    for i in 0..alen {
        if &a.get_item(py, i).extract::<PyBytes>(py)?.data(py)[1..]
            != b.get_item(py, i + bstart).extract::<PyBytes>(py)?.data(py)
        {
            return Ok(-1);
        }
    }
    Ok(0)
}

fn to_object(py: Python, s: &[u8]) -> PyObject {
    PyBytes::new(py, s).into_object()
}
