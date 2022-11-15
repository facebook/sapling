/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;
use std::ops::Bound;

use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::ResultPyErrExt;
use indexedlog::log::IndexDef;
use indexedlog::log::IndexOutput;
use indexedlog::log::LogLookupIter;
use indexedlog::log::LogRangeIter;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "indexedlog"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<Log>(py)?;
    m.add_class::<OpenOptions>(py)?;
    Ok(m)
}

py_class!(class OpenOptions |py| {
    data defs: RefCell<Vec<IndexDef>>;

    def __new__(_cls) -> PyResult<Self> {
        Self::create_instance(py, RefCell::new(Vec::new()))
    }

    /// index_def(name: str, func: Callable[[bytes], List[bytes | range]], lag_threshold=None)
    ///
    /// Append an index definition defined by `func`.
    /// `func` takes a byte slice (entry to insert)
    def index_def(&self, name: String, func: PyObject, lag_threshold: Option<u64> = None) -> PyResult<Self> {
        let mut defs = self.defs(py).borrow_mut();
        let func = to_index_func(py, func);
        let mut def = IndexDef::new(name, func);
        if let Some(lag) = lag_threshold {
            def = def.lag_threshold(lag);
        }
        defs.push(def);
        Ok(self.clone_ref(py))
    }

    /// open(path, crate=True) -> Log
    def open(&self, dir: &PyPath, create: bool = true) -> PyResult<Log> {
        let dir = dir.as_path();
        let defs = self.defs(py).borrow().clone();
        let opts = indexedlog::log::OpenOptions::new().create(create).index_defs(defs);
        let log = opts.open(dir).map_pyerr(py)?;
        Log::create_instance(py, RefCell::new(log))
    }
});

py_class!(class Log |py| {
    data log: RefCell<indexedlog::log::Log>;

    def __new__(_cls, path: &PyPath) -> PyResult<Self> {
        let index_defs = Vec::new();
        let log = indexedlog::log::Log::open(path, index_defs).map_pyerr(py)?;
        Self::create_instance(py, RefCell::new(log))
    }

    /// entries(skip=0, take=sys.maxsize, dirty=False) -> List[memoryview]
    ///
    /// Get all entries in the Log. The entries are wrapped in a zero-copy
    /// `Bytes` type that requires `asref()` to get the underlying bytes
    /// as a memoryview.
    def entries(&self, skip: usize=0, take: usize=usize::MAX, dirty: bool=false) -> PyResult<Vec<pybytes::Bytes>> {
        let log = self.log(py).borrow();
        let iter = if dirty { log.iter_dirty() } else { log.iter() };
        let items: Vec<&[u8]> = iter.skip(skip).take(take).collect::<Result<Vec<_>, _>>().map_pyerr(py)?;
        let items: Vec<pybytes::Bytes> = items.into_iter().map(|s| {
            pybytes::Bytes::from_bytes(py, log.slice_to_bytes(s))
        }).collect::<Result<_, _>>()?;
        Ok(items)
    }

    /// Append an entry to the Log.
    def append(&self, data: PyBytes) -> PyResult<PyNone> {
        let mut log = self.log(py).borrow_mut();
        let data = data.data(py);
        log.append(data).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// lookup(index_id: int, key: bytes) -> List[bytes]
    def lookup(&self, index_id: usize, key: PyBytes) -> PyResult<Vec<PyBytes>> {
        let log = self.log(py).borrow();
        let key = key.data(py);
        let iter = log.lookup(index_id, key).map_pyerr(py)?;
        let result = lookup_iter_to_vec(py, iter).map_pyerr(py)?;
        Ok(result)
    }

    /// lookup_prefix(index_id: int, prefix: bytes) -> List[Tuple[bytes, List[bytes]]]
    def lookup_prefix(&self, index_id: usize, prefix: PyBytes) -> PyResult<Vec<(PyBytes, Vec<PyBytes>)>> {
        let log = self.log(py).borrow();
        let prefix = prefix.data(py);
        let iter = log.lookup_prefix(index_id, prefix).map_pyerr(py)?;
        let result = range_iter_to_vec(py, iter).map_pyerr(py)?;
        Ok(result)
    }

    /// lookup_prefix_hex(index_id: int, hex_prefix: bytes) -> List[Tuple[bytes, List[bytes]]]
    def lookup_prefix_hex(&self, index_id: usize, hex_prefix: PyBytes) -> PyResult<Vec<(PyBytes, Vec<PyBytes>)>> {
        let log = self.log(py).borrow();
        let hex_prefix = hex_prefix.data(py);
        let iter = log.lookup_prefix_hex(index_id, hex_prefix).map_pyerr(py)?;
        let result = range_iter_to_vec(py, iter).map_pyerr(py)?;
        Ok(result)
    }

    /// lookup_range(index_id: int, start: bytes, end: bytes, start_inclusive: bool=True, end_inclusive: bool=False) -> List[Tuple[bytes, List[bytes]]]
    def lookup_range(&self, index_id: usize, start: Option<PyBytes> = None, end: Option<PyBytes> = None, start_inclusive: bool = true, end_inclusive: bool = false) -> PyResult<Vec<(PyBytes, Vec<PyBytes>)>> {
        let log = self.log(py).borrow();
        let range = (to_bound(py, start.as_ref(), start_inclusive), to_bound(py, end.as_ref(), end_inclusive));
        let iter = log.lookup_range(index_id, range).map_pyerr(py)?;
        let result = range_iter_to_vec(py, iter).map_pyerr(py)?;
        Ok(result)
    }

    /// Write pending changes to disk and pick up changes from disk.
    def sync(&self) -> PyResult<PyNone> {
        let mut log = self.log(py).borrow_mut();
        log.sync().map_pyerr(py)?;
        Ok(PyNone)
    }
});

fn to_bound<'a>(py: Python, key: Option<&'a PyBytes>, inclusive: bool) -> Bound<&'a [u8]> {
    match key {
        None => Bound::Unbounded,
        Some(key) => {
            let key = key.data(py);
            match inclusive {
                true => Bound::Included(key),
                false => Bound::Excluded(key),
            }
        }
    }
}

fn to_index_func(
    py: Python,
    func: PyObject,
) -> impl Fn(&[u8]) -> Vec<IndexOutput> + Send + Sync + 'static {
    let func = func.clone_ref(py);
    move |data: &[u8]| -> Vec<IndexOutput> {
        let gil_guard = Python::acquire_gil();
        let py = gil_guard.python();
        let data = PyBytes::new(py, data);
        let result = func.call(py, (data,), None).unwrap();
        let result: Vec<PyObject> = result.extract(py).unwrap();
        result
            .into_iter()
            .map(|obj| {
                if let Ok(obj) = obj.extract::<PyBytes>(py) {
                    let out: &[u8] = obj.data(py);
                    return IndexOutput::Owned(out.to_vec().into_boxed_slice());
                } else if let Ok((start, end)) = obj.extract::<(u64, u64)>(py) {
                    return IndexOutput::Reference(start..end);
                } else {
                    panic!("python index function returned unknown value")
                }
            })
            .collect()
    }
}

fn lookup_iter_to_vec(py: Python, iter: LogLookupIter) -> indexedlog::Result<Vec<PyBytes>> {
    let result = iter.collect::<Result<Vec<_>, _>>()?;
    let result = result
        .into_iter()
        .map(|item| PyBytes::new(py, item))
        .collect();
    Ok(result)
}

fn range_iter_to_vec(
    py: Python,
    iter: LogRangeIter,
) -> indexedlog::Result<Vec<(PyBytes, Vec<PyBytes>)>> {
    let result = iter.collect::<Result<Vec<_>, _>>()?;
    let result = result
        .into_iter()
        .map(
            |(full_key, iter)| -> indexedlog::Result<(PyBytes, Vec<PyBytes>)> {
                Ok((PyBytes::new(py, &full_key), lookup_iter_to_vec(py, iter)?))
            },
        )
        .collect::<Result<Vec<_>, _>>()?;
    Ok(result)
}
