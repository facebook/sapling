/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "linelog"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<IntLineLog>(py)?;
    Ok(m)
}

// Line content is "int", not "str".
type NativeIntLineLog = ::linelog::AbstractLineLog<usize>;

py_class!(class IntLineLog |py| {
    data inner: NativeIntLineLog;

    def __new__(_cls) -> PyResult<Self> {
        Self::create_instance(py, Default::default())
    }

    /// Get the maximum rev (inclusive).
    def max_rev(&self) -> PyResult<usize> {
        Ok(self.inner(py).max_rev())
    }

    /// Edit chunk (a_rev, a1, a2, b_rev, b1, b2) -> self.
    def edit_chunk(&self, a_rev: usize, a1: usize, a2: usize, b_rev: usize, b1: usize, b2: usize) -> PyResult<Self> {
        let inner = self.inner(py);
        let b_lines = (b1..b2).collect::<Vec<_>>();
        let new_value = inner.clone().edit_chunk(a_rev, a1, a2, b_rev, b_lines);
        Self::create_instance(py, new_value)
    }

    /// Get the lines. (rev, start_rev=rev) -> [(rev, line_no, pc, deleted)].
    /// Includes a dummy "end" line at the end.
    def checkout_lines(&self, rev: usize, start_rev: Option<usize> = None) -> PyResult<Vec<(usize, usize, usize, bool)>> {
        let inner = self.inner(py);
        let lines = match start_rev {
            None => inner.checkout_lines(rev),
            Some(start) => inner.checkout_range_lines(start, rev),
        };
        let lines: Vec<_> = lines.into_iter().map(|l| (l.rev, *l.data.as_ref(), l.pc, l.deleted)).collect();
        Ok(lines)
    }
});
