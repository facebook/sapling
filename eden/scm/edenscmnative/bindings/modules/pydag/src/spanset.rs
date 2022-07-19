/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;

use cpython::*;
use dag::Id;
use dag::IdSet;
use dag::IdSetIter;

/// A wrapper around [`IdSet`] with Python integration.
///
/// Differences from the `py_class` version:
/// - Auto converts from a wider range of Python types - smartset, any iterator.
/// - No need to take the Python GIL to create a new instance of `Set`.
pub struct Spans(pub IdSet);

impl Into<IdSet> for Spans {
    fn into(self) -> IdSet {
        self.0
    }
}

// A wrapper around [`IdSet`].
// This is different from `smartset.spanset`.
// Used in the Python world. The Rust world should use the `Spans` and `IdSet` types.
py_class!(pub class spans |py| {
    data inner: IdSet;

    def __new__(_cls, obj: PyObject) -> PyResult<spans> {
        Ok(Spans::extract(py, &obj)?.to_py_object(py))
    }

    /// Construct a 'spans' from an arbitrary integer range.
    ///
    /// This is unsafe because there are no validation that Ids in this
    /// range are valid.
    ///
    /// Use `range & torevs(dag.all())` to get a valid Set.
    ///
    /// This should only be used to be compatible with legacy revsets like
    /// "x:", ":y", "x:y", ":", or for fast paths of lazy sets (ex. ancestors
    /// with a cutoff minrev). Avoid using this function in new code.
    @staticmethod
    def unsaferange(start: Option<i64> = None, end: Option<i64> = None) -> PyResult<Spans> {
        let _ = py;
        if end.unwrap_or(0) < 0 {
            return Ok(Spans(IdSet::empty()))
        }
        let start = match start {
            Some(start) => Id(start.max(0) as u64),
            None => Id::MIN,
        };
        let end = match end {
            Some(end) => Id(end.max(0) as u64),
            None => Id::MAX,
        };
        let id_set: IdSet = if start <= end {
            IdSet::from_spans(vec![start..=end])
        } else {
            IdSet::empty()
        };
        Ok(Spans(id_set))
    }

    def __contains__(&self, id: i64) -> PyResult<bool> {
        if id < 0 {
            Ok(false)
        } else {
            Ok(self.inner(py).contains(Id(id as u64)))
        }
    }

    def __len__(&self) -> PyResult<usize> {
        Ok(self.inner(py).count() as usize)
    }

    def __iter__(&self) -> PyResult<spansiter> {
        self.iterdesc(py)
    }

    def iterasc(&self) -> PyResult<spansiter> {
        let iter = RefCell::new( self.inner(py).clone().into_iter());
        spansiter::create_instance(py, iter, true)
    }

    def iterdesc(&self) -> PyResult<spansiter> {
        let iter = RefCell::new(self.inner(py).clone().into_iter());
        spansiter::create_instance(py, iter, false)
    }

    def min(&self) -> PyResult<Option<u64>> {
        Ok(self.inner(py).min().map(|id| id.0))
    }

    def max(&self) -> PyResult<Option<u64>> {
        Ok(self.inner(py).max().map(|id| id.0))
    }

    def __repr__(&self) -> PyResult<String> {
        Ok(format!("[{:?}]", self.inner(py)))
    }

    def __add__(lhs, rhs) -> PyResult<Spans> {
        let lhs = Spans::extract(py, lhs)?;
        let rhs = Spans::extract(py, rhs)?;
        Ok(Spans(lhs.0.union(&rhs.0)))
    }

    def __and__(lhs, rhs) -> PyResult<Spans> {
        let lhs = Spans::extract(py, lhs)?;
        let rhs = Spans::extract(py, rhs)?;
        Ok(Spans(lhs.0.intersection(&rhs.0)))
    }

    def __sub__(lhs, rhs) -> PyResult<Spans> {
        let lhs = Spans::extract(py, lhs)?;
        let rhs = Spans::extract(py, rhs)?;
        Ok(Spans(lhs.0.difference(&rhs.0)))
    }
});

// A wrapper to [`IdSetIter`].
py_class!(pub class spansiter |py| {
    data iter: RefCell<IdSetIter<IdSet>>;
    data ascending: bool;

    def __next__(&self) -> PyResult<Option<u64>> {
        let mut iter = self.iter(py).borrow_mut();
        let next = if *self.ascending(py) {
            iter.next_back()
        } else {
            iter.next()
        };
        Ok(next.map(|id| id.0))
    }

    // Makes code like `list(spans.iterasc())` work.
    def __iter__(&self) -> PyResult<spansiter> {
        Ok(self.clone_ref(py))
    }
});

impl<'a> FromPyObject<'a> for Spans {
    fn extract(py: Python, obj: &'a PyObject) -> PyResult<Self> {
        // If obj already owns Set, then avoid iterating through it.
        if let Ok(pyset) = obj.extract::<spans>(py) {
            return Ok(Spans(pyset.inner(py).clone()));
        }

        // Then iterate through obj and collect all ids.
        // Collecting ids to a Vec first to preserve error handling.
        let ids: PyResult<Vec<Id>> = obj
            .iter(py)?
            .map(|o| Ok(o?.extract::<Option<i64>>(py)?))
            .filter_map(|o| match o {
                // Skip "None" (wdir?) automatically.
                Ok(None) => None,
                Ok(Some(i)) => {
                    // Skip "nullrev" automatically.
                    if i >= 0 { Some(Ok(Id(i as u64))) } else { None }
                }
                Err(e) => Some(Err(e)),
            })
            .collect();
        Ok(Spans(IdSet::from_spans(ids?)))
    }
}

impl ToPyObject for Spans {
    type ObjectType = spans;

    fn to_py_object(&self, py: Python) -> Self::ObjectType {
        spans::create_instance(py, self.0.clone()).unwrap()
    }
}
