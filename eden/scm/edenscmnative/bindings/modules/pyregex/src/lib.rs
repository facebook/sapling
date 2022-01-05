/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::collections::HashMap;
use std::ops::Range;

use cpython::*;
use cpython_ext::ResultPyErrExt;
use cpython_ext::Str;
use regex::bytes::Regex as BinaryRegex;
use regex::Regex;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "regex"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "compile", py_fn!(py, compile(s: PyObject)))?;
    m.add(py, "escape", py_fn!(py, escape(s: &str)))?;
    Ok(m)
}

fn compile(py: Python, s: PyObject) -> PyResult<PyObject> {
    if let Ok(bytes) = s.extract::<PyBytes>(py) {
        let text = std::str::from_utf8(bytes.data(py)).map_pyerr(py)?;
        Ok(BytesPattern::compile(py, text)?.into_object())
    } else if let Ok(text) = s.extract::<String>(py) {
        Ok(StringPattern::compile(py, &text)?.into_object())
    } else {
        Err(PyErr::new::<exc::TypeError, _>(
            py,
            "compile requires str or bytes",
        ))
    }
}

fn escape(_py: Python, s: &str) -> PyResult<Str> {
    Ok(regex::escape(s).into())
}

py_class!(class StringPattern |py| {
    data re: Regex;
    data match_re: Regex;
    data raw_pattern: String;

    @staticmethod
    def compile(s: &str) -> PyResult<Self> {
        let re = Regex::new(s).map_pyerr(py)?;
        // match_re is used for the Python "match" API, which aligns the start, but not the end.
        // (Rust regex does not have such equivalent).
        let match_re = Regex::new(&format!(r"\A{}", s)).map_pyerr(py)?;
        let raw_pattern = s.to_string();
        Self::create_instance(py, re, match_re, raw_pattern)
    }

    def search(&self, s: PyString) -> PyResult<Option<StringMatchObject>> {
        StringMatchObject::new(py, self.re(py), s, self.clone_ref(py))
    }

    def r#match(&self, s: PyString) -> PyResult<Option<StringMatchObject>> {
        StringMatchObject::new(py, self.match_re(py), s, self.clone_ref(py))
    }

    def __repr__(&self) -> PyResult<String> {
        Ok(format!("<StringPattern {:?}>", self.raw_pattern(py)))
    }
});

py_class!(class StringMatchObject |py| {
    data ranges: Vec<Option<Range<usize>>>;
    data text: PyString;
    data dict: HashMap<String, Range<usize>>;

    def group(&self, i: usize = 0) -> PyResult<Option<Str>> {
        match self.get_range(py, i)? {
            None => Ok(None),
            Some(range) => Ok(Some(self.get_slice(py, range)?)),
        }
    }

    def groups(&self) -> PyResult<Vec<Option<Str>>> {
        self.ranges(py).iter().skip(1).cloned().map(|range| {
            range.map(|r| self.get_slice(py, r)).transpose()
        }).collect::<PyResult<_>>()
    }

    def groupdict(&self) -> PyResult<HashMap<Str, Str>> {
        let mut dict = HashMap::new();
        for (name, range) in self.dict(py).iter() {
            dict.insert(name.to_string().into(), self.get_slice(py, range.clone())?);
        }
        Ok(dict)
    }

    def start(&self, i: usize = 0) -> PyResult<Option<usize>> {
        Ok(self.get_range(py, i)?.map(|r| r.start))
    }

    def end(&self, i: usize = 0) -> PyResult<Option<usize>> {
        Ok(self.get_range(py, i)?.map(|r| r.end))
    }

    def span(&self, i: usize = 0) -> PyResult<Option<(usize, usize)>> {
        let range = self.get_range(py, i)?;
        Ok(range.map(|r| (r.start, r.end)))
    }
});

impl StringMatchObject {
    fn get_range(&self, py: Python, i: usize) -> PyResult<Option<Range<usize>>> {
        match self.ranges(py).get(i) {
            None => Err(PyErr::new::<exc::IndexError, _>(py, "no such group")),
            Some(range) => Ok(range.clone()),
        }
    }

    fn get_slice(&self, py: Python, range: Range<usize>) -> PyResult<Str> {
        let slice = self
            .text(py)
            .to_string(py)
            .expect("should not error since it was verified by 'StringMatchObject::new'")
            .get(range)
            .ok_or_else(|| {
                PyErr::new::<exc::IndexError, _>(
                    py,
                    "unexpected str slice (possibly a bug in the Rust regex crate)",
                )
            })?
            .to_string();
        Ok(slice.into())
    }

    fn new(
        py: Python,
        re: &Regex,
        pystr: PyString,
        pattern: StringPattern,
    ) -> PyResult<Option<Self>> {
        let s = pystr.to_string(py)?;
        re.captures(&s)
            .map(|c| {
                let ranges = (0..c.len()).map(|i| c.get(i).map(|r| r.range())).collect();
                let mut dict = HashMap::new();
                for name in pattern.re(py).capture_names() {
                    if let Some(name) = name {
                        if let Some(m) = c.name(name) {
                            dict.insert(name.to_string(), m.range());
                        }
                    }
                }
                StringMatchObject::create_instance(py, ranges, pystr.clone_ref(py), dict)
            })
            .transpose()
    }
}

py_class!(class BytesPattern |py| {
    data re: BinaryRegex;
    data match_re: BinaryRegex;
    data raw_pattern: String;

    @staticmethod
    def compile(s: &str) -> PyResult<Self> {
        let re = BinaryRegex::new(s).map_pyerr(py)?;
        // For Python "match" API - only search the beginning.
        let match_re = BinaryRegex::new(&format!(r"\A{}", s)).map_pyerr(py)?;
        let raw_pattern = s.to_string();
        Self::create_instance(py, re, match_re, raw_pattern)
    }

    def search(&self, s: PyBytes) -> PyResult<Option<BytesMatchObject>> {
        BytesMatchObject::new(py, self.re(py), s, self.clone_ref(py))
    }

    def r#match(&self, s: PyBytes) -> PyResult<Option<BytesMatchObject>> {
        BytesMatchObject::new(py, self.match_re(py), s, self.clone_ref(py))
    }

    def __repr__(&self) -> PyResult<String> {
        Ok(format!("<BytesPattern {:?}>", self.raw_pattern(py)))
    }
});

py_class!(class BytesMatchObject |py| {
    data ranges: Vec<Option<Range<usize>>>;
    data text: PyBytes;
    data dict: HashMap<String, Range<usize>>;

    def group(&self, i: usize = 0) -> PyResult<Option<PyBytes>> {
        match self.get_range(py, i)? {
            None => Ok(None),
            Some(range) => Ok(Some(self.get_slice(py, range)?)),
        }
    }

    def groups(&self) -> PyResult<Vec<Option<PyBytes>>> {
        self.ranges(py).iter().skip(1).cloned().map(|range| {
            range.map(|r| self.get_slice(py, r)).transpose()
        }).collect::<PyResult<_>>()
    }

    def groupdict(&self) -> PyResult<HashMap<String, PyBytes>> {
        let mut dict = HashMap::new();
        for (name, range) in self.dict(py).iter() {
            dict.insert(name.to_string(), self.get_slice(py, range.clone())?);
        }
        Ok(dict)
    }

    def start(&self, i: usize = 0) -> PyResult<Option<usize>> {
        Ok(self.get_range(py, i)?.map(|r| r.start))
    }

    def end(&self, i: usize = 0) -> PyResult<Option<usize>> {
        Ok(self.get_range(py, i)?.map(|r| r.end))
    }

    def span(&self, i: usize = 0) -> PyResult<Option<(usize, usize)>> {
        let range = self.get_range(py, i)?;
        Ok(range.map(|r| (r.start, r.end)))
    }
});

impl BytesMatchObject {
    fn get_range(&self, py: Python, i: usize) -> PyResult<Option<Range<usize>>> {
        match self.ranges(py).get(i) {
            None => Err(PyErr::new::<exc::IndexError, _>(py, "no such group")),
            Some(range) => Ok(range.clone()),
        }
    }

    fn get_slice(&self, py: Python, range: Range<usize>) -> PyResult<PyBytes> {
        Ok(PyBytes::new(py, &self.text(py).data(py)[range]))
    }

    fn new(
        py: Python,
        re: &BinaryRegex,
        pybytes: PyBytes,
        pattern: BytesPattern,
    ) -> PyResult<Option<Self>> {
        re.captures(pybytes.data(py))
            .map(|c| {
                let ranges = (0..c.len()).map(|i| c.get(i).map(|c| c.range())).collect();
                let mut dict = HashMap::new();
                for name in pattern.re(py).capture_names() {
                    if let Some(name) = name {
                        if let Some(m) = c.name(name) {
                            dict.insert(name.to_string(), m.range());
                        }
                    }
                }
                BytesMatchObject::create_instance(py, ranges, pybytes.clone_ref(py), dict)
            })
            .transpose()
    }
}
