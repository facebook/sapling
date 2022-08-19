/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;

use configparser::config::ConfigSet;
use configparser::config::Options;
use configparser::config::SupersetVerification;
use configparser::convert::parse_list;
use configparser::hg::ConfigSetHgExt;
use configparser::hg::OptionsHgExt;
use cpython::*;
use cpython_ext::error::Result;
use cpython_ext::error::ResultPyErrExt;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use cpython_ext::Str;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "configparser"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<config>(py)?;
    m.add(py, "parselist", py_fn!(py, parselist(value: String)))?;
    Ok(m)
}

py_class!(pub class config |py| {
    data cfg: RefCell<ConfigSet>;

    def __new__(_cls) -> PyResult<config> {
        config::create_instance(py, RefCell::new(ConfigSet::new()))
    }

    def clone(&self) -> PyResult<config> {
        let cfg = self.cfg(py).borrow();
        config::create_instance(py, RefCell::new(cfg.clone()))
    }

    def readpath(
        &self,
        path: &PyPath,
        source: String,
        sections: Option<Vec<String>>,
        remap: Option<Vec<(String, String)>>,
        readonly_items: Option<Vec<(String, String)>>
    ) -> PyResult<Vec<Str>> {
        let mut cfg = self.cfg(py).borrow_mut();

        let mut opts = Options::new().source(source).process_hgplain();
        if let Some(sections) = sections {
            opts = opts.filter_sections(sections);
        }
        if let Some(remap) = remap {
            let map = remap.into_iter().collect();
            opts = opts.remap_sections(map);
        }
        if let Some(readonly_items) = readonly_items {
            opts = opts.readonly_items(readonly_items);
        }

        let errors = cfg.load_path(path, &opts);
        Ok(errors_to_str_vec(errors))
    }

    def parse(&self, content: String, source: String) -> PyResult<Vec<Str>> {
        let mut cfg = self.cfg(py).borrow_mut();
        let opts = source.into();
        let errors = cfg.parse(content, &opts);
        Ok(errors_to_str_vec(errors))
    }

    def get(&self, section: String, name: String) -> PyResult<Option<PyUnicode>> {
        let cfg = self.cfg(py).borrow();

        Ok(cfg.get(section, name).map(|v| PyUnicode::new(py, &v)))
    }

    def sources(
        &self, section: String, name: String
    ) -> PyResult<Vec<(Option<PyUnicode>, Option<(PyPathBuf, usize, usize, usize)>, PyUnicode)>> {
        // Return [(value, file_source, source)]
        // file_source is a tuple of (file_path, byte_start, byte_end, line)
        let cfg = self.cfg(py).borrow();
        let sources = cfg.get_sources(section, name);
        let mut result = Vec::with_capacity(sources.len());
        for source in sources {
            let value = source.value().as_ref().map(|v| PyUnicode::new(py, &v));
            let file = source.location().map(|(path, range)| {
                // Calculate the line number - count "\n" till range.start
                let file = source.file_content().unwrap();
                let line = 1 + file.slice(0..range.start).chars().filter(|ch| *ch == '\n').count();

                let pypath = if path.as_os_str().is_empty() {
                    PyPathBuf::from(String::from("<builtin>"))
                } else {
                    let path = util::path::strip_unc_prefix(&path);
                    path.try_into().unwrap()
                };
                (pypath, range.start, range.end, line)
            });
            let source = PyUnicode::new(py, &source.source());
            result.push((value, file, source));
        }
        Ok(result)
    }

    def set(
        &self, section: String, name: String, value: Option<String>, source: String
    ) -> PyResult<PyNone> {
        let mut cfg = self.cfg(py).borrow_mut();
        let opts = source.into();
        cfg.set(section, name, value, &opts);
        Ok(PyNone)
    }

    def sections(&self) -> PyResult<Vec<PyUnicode>> {
        let cfg = self.cfg(py).borrow();
        Ok(cfg.sections().iter().map(|s| PyUnicode::new(py, &s)).collect())
    }

    def names(&self, section: String) -> PyResult<Vec<PyUnicode>> {
        let cfg = self.cfg(py).borrow();
        Ok(cfg.keys(section).iter().map(|s| PyUnicode::new(py, &s)).collect())
    }

    def tostring(&self) -> PyResult<Str> {
        let cfg = self.cfg(py).borrow();
        Ok(cfg.to_string().into())
    }

    @staticmethod
    def load(repopath: Option<PyPathBuf>) -> PyResult<(config, Vec<(Str, Str, Option<Str>, Option<Str>)>)> {
        let repopath = repopath.as_ref().map(|p| p.as_path());
        let mut cfg = ConfigSet::new();
        let results = cfg.load::<String, String>(repopath, None).map_pyerr(py)?;
        Ok((
            config::create_instance(py, RefCell::new(cfg))?,
            convert_superset_verification(results)
        ))
    }

    def reload(
        &self,
        repopath: Option<PyPathBuf>,
        readonly_items: Option<Vec<(String, String)>>
    ) -> PyResult<Vec<(Str, Str, Option<Str>, Option<Str>)>> {
        let repopath = repopath.as_ref().map(|p| p.as_path());
        let mut cfg = self.cfg(py).borrow_mut();
        let results = cfg.load(repopath, readonly_items).map_pyerr(py)?;
        Ok(convert_superset_verification(results))
    }

    def files(&self) -> PyResult<Vec<PyPathBuf>> {
        self.cfg(py).borrow().files().iter().map(|p| p.as_path().try_into()).collect::<Result<Vec<PyPathBuf>>>().map_pyerr(py)
    }

    def validate(&self) -> PyResult<Vec<(Str, Str, Option<Str>, Option<Str>)>> {
        let mut cfg = self.cfg(py).borrow_mut();
        let results = cfg.validate_dynamic().map_pyerr(py)?;
        Ok(convert_superset_verification(results))
    }
});

fn convert_superset_verification(
    results: SupersetVerification,
) -> Vec<(Str, Str, Option<Str>, Option<Str>)> {
    let mut output: Vec<(Str, Str, Option<Str>, Option<Str>)> = vec![];
    for ((section, key), value) in results.missing.iter() {
        output.push((
            section.to_string().into(),
            key.to_string().into(),
            None,
            Some(value.to_string().into()),
        ));
    }

    for ((section, key), value) in results.extra.iter() {
        output.push((
            section.to_string().into(),
            key.to_string().into(),
            Some(value.to_string().into()),
            None,
        ));
    }

    for ((section, key), super_value, sub_value) in results.mismatched.iter() {
        output.push((
            section.to_string().into(),
            key.to_string().into(),
            Some(super_value.to_string().into()),
            Some(sub_value.to_string().into()),
        ));
    }

    output
}

impl config {
    pub fn get_cfg(&self, py: Python) -> ConfigSet {
        self.cfg(py).clone().into_inner()
    }
}

fn parselist(py: Python, value: String) -> PyResult<Vec<PyUnicode>> {
    Ok(parse_list(value)
        .iter()
        .map(|v| PyUnicode::new(py, &v))
        .collect())
}

fn errors_to_str_vec(errors: Vec<configparser::error::Error>) -> Vec<Str> {
    errors
        .into_iter()
        .map(|err| Str::from(format!("{}", err)))
        .collect()
}
