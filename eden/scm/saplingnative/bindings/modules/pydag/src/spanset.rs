/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;
use std::sync::OnceLock;

use cpython::*;
use dag::Id;
use dag::IdList;
use dag::IdSet;
use dag::IdSetIter;
use dag::OrderedSpan;
use types::hgid::WDIR_REV;

/// A wrapper around [`IdSet`] with Python integration.
///
/// Differences from the `py_class` version:
/// - Auto converts from a wider range of Python types - smartset, any iterator.
///   Attempt to preserve order. The iterator item could be int or (int, int).
///   The latter represents an OrderedSpan.
/// - No need to take the Python GIL to create a new instance of `Set`.
#[derive(Clone)]
pub enum Spans {
    // Without iteration order.
    Set(IdSet),
    // With iteration order.
    List(IdList, OnceLock<IdSet>),
}

impl From<Spans> for IdSet {
    fn from(val: Spans) -> Self {
        match val {
            Spans::Set(s) => s,
            Spans::List(l, mut s) => match s.take() {
                Some(s) => s,
                None => l.to_set(),
            },
        }
    }
}

impl Spans {
    pub fn from_id_set(id_set: IdSet) -> Self {
        Self::Set(id_set)
    }

    pub fn from_id_list(id_list: IdList) -> Self {
        Self::List(id_list, OnceLock::new())
    }

    pub fn as_id_set(&self) -> &IdSet {
        match self {
            Spans::Set(s) => s,
            Spans::List(l, s) => s.get_or_init(|| l.to_set()),
        }
    }

    pub fn maybe_as_id_list(&self) -> Option<&IdList> {
        match self {
            Spans::Set(_) => None,
            Spans::List(l, _) => Some(l),
        }
    }

    /// Drop order preserving behavior.
    pub fn drop_order(&mut self) -> &mut Self {
        match self {
            Spans::Set(_) => {}
            Spans::List(l, s) => {
                let id_set = match s.take() {
                    Some(s) => s,
                    None => l.to_set(),
                };
                *self = Self::from_id_set(id_set)
            }
        }
        self
    }
}

// A wrapper around [`IdSet`].
// This is different from `smartset.spanset`.
// Used in the Python world. The Rust world should use the `Spans` and `IdSet` types.
py_class!(pub class spans |py| {
    data inner: Spans;

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
            return Ok(Spans::from_id_set(IdSet::empty()))
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
        Ok(Spans::from_id_set(id_set))
    }

    def __contains__(&self, id: i64) -> PyResult<bool> {
        if id < 0 {
            Ok(false)
        } else {
            Ok(self.as_id_set(py).contains(Id(id as u64)))
        }
    }

    def __len__(&self) -> PyResult<usize> {
        Ok(self.as_id_set(py).count() as usize)
    }

    def __iter__(&self) -> PyResult<spansiter> {
        self.iterdesc(py)
    }

    def iterasc(&self) -> PyResult<spansiter> {
        // XXX: This does not ocnsider the List case.
        let iter = RefCell::new( self.as_id_set(py).clone().into_iter());
        spansiter::create_instance(py, iter, true)
    }

    def iterdesc(&self) -> PyResult<spansiter> {
        // XXX: This does not ocnsider the List case.
        let iter = RefCell::new(self.as_id_set(py).clone().into_iter());
        spansiter::create_instance(py, iter, false)
    }

    def min(&self) -> PyResult<Option<u64>> {
        Ok(self.as_id_set(py).min().map(|id| id.0))
    }

    def max(&self) -> PyResult<Option<u64>> {
        Ok(self.as_id_set(py).max().map(|id| id.0))
    }

    def __repr__(&self) -> PyResult<String> {
        // XXX: This does not ocnsider the List case.
        Ok(format!("[{:?}]", self.as_id_set(py)))
    }

    def __add__(lhs, rhs) -> PyResult<Spans> {
        let lhs = Spans::extract(py, lhs)?;
        let rhs = Spans::extract(py, rhs)?;
        Ok(Spans::from_id_set(lhs.as_id_set().union(rhs.as_id_set())))
    }

    def __and__(lhs, rhs) -> PyResult<Spans> {
        let lhs = Spans::extract(py, lhs)?;
        let rhs = Spans::extract(py, rhs)?;
        Ok(Spans::from_id_set(lhs.as_id_set().intersection(rhs.as_id_set())))
    }

    def __sub__(lhs, rhs) -> PyResult<Spans> {
        let lhs = Spans::extract(py, lhs)?;
        let rhs = Spans::extract(py, rhs)?;
        Ok(Spans::from_id_set(lhs.as_id_set().difference(rhs.as_id_set())))
    }
});

impl spans {
    fn as_id_set<'a>(&'a self, py: Python<'a>) -> &'a IdSet {
        self.inner(py).as_id_set()
    }
}

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

fn python_rev_to_id(rev: i64) -> Option<Id> {
    // Skip "nullrev" (-1) and "wdirrev" (0x7FFFFFFFFFFFFFFF) automatically for now.
    if rev >= 0 && rev != WDIR_REV {
        Some(Id(rev as u64))
    } else {
        None
    }
}

impl<'a> FromPyObject<'a> for Spans {
    fn extract(py: Python, obj: &'a PyObject) -> PyResult<Self> {
        // If obj already owns Set, then avoid iterating through it.
        if let Ok(pyset) = obj.extract::<spans>(py) {
            let set = pyset.inner(py).clone();
            return Ok(set.clone());
        }

        // Then iterate through obj and collect all ids.
        // Collecting ids to a Vec first to preserve error handling.
        let spans: PyResult<Vec<OrderedSpan>> = obj
            .iter(py)?
            .filter_map(|o| {
                let o = match o {
                    Err(e) => return Some(Err(e)),
                    Ok(v) => v,
                };
                match o.extract::<Option<i64>>(py) {
                    // Skip "None" (wdir?) automatically.
                    Ok(None) => None,
                    Ok(Some(i)) => {
                        python_rev_to_id(i).map(|id| Ok(OrderedSpan { start: id, end: id }))
                    }
                    Err(e) => match o.extract::<(i64, i64)>(py) {
                        Ok((start, end)) => {
                            if let (Some(start), Some(end)) =
                                (python_rev_to_id(start), python_rev_to_id(end))
                            {
                                Some(Ok(OrderedSpan { start, end }))
                            } else {
                                None
                            }
                        }
                        Err(_) => Some(Err(e)),
                    },
                }
            })
            .collect();

        let id_list = IdList::from_spans(spans?);

        Ok(Spans::from_id_list(id_list))
    }
}

impl ToPyObject for Spans {
    type ObjectType = spans;

    fn to_py_object(&self, py: Python) -> Self::ObjectType {
        spans::create_instance(py, self.clone()).unwrap()
    }
}
