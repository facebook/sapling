/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;

// Read-only representation of a Rust dag::VerLink.
py_class!(pub class VerLink |py| {
    data inner: dag::VerLink;

    def __repr__(&self) -> PyResult<String> {
        Ok(format!("<VerLink {:?}>", self.inner(py)))
    }

    /// cmp(rhs): Partial order between 2 VerLinks.
    /// -1: self < rhs; 0: self == rhs; 1: self > rhs; None: not comparable
    def cmp(&self, rhs: VerLink) -> PyResult<Option<i8>> {
        use std::cmp::Ordering;

        let ord = self.inner(py).partial_cmp(rhs.inner(py));
        let ord = ord.map(|ord| match ord {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        });
        Ok(ord)
    }

    def __richcmp__(&self, rhs: VerLink, op: CompareOp) -> PyResult<bool> {
        use std::cmp::Ordering::*;
        use CompareOp::*;

        let ord = self.inner(py).partial_cmp(rhs.inner(py));
        let result = match ord {
            None => matches!(op, Ne),
            Some(Less) => matches!(op, Lt | Le | Ne),
            Some(Greater) => matches!(op, Gt | Ge | Ne),
            Some(Equal) => matches!(op, Le | Ge | Eq),
        };
        Ok(result)
    }
});
