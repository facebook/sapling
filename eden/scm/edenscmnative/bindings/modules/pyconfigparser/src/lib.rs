/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::{cell::RefCell, collections::HashSet, convert::TryInto, iter::FromIterator};

use cpython::*;

use configparser::{
    config::{ConfigSet, Options},
    dynamicconfig::Generator,
    hg::{generate_dynamicconfig, parse_list, ConfigSetHgExt, OptionsHgExt},
};
use cpython_ext::{error::ResultPyErrExt, PyNone, PyPath, PyPathBuf, Str};

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "configparser"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<config>(py)?;
    m.add(py, "parselist", py_fn!(py, parselist(value: String)))?;
    m.add(
        py,
        "applydynamicconfig",
        py_fn!(
            py,
            applydynamicconfig(config: config, repo_name: String, shared_path: PyPathBuf)
        ),
    )?;
    m.add(
        py,
        "generatedynamicconfig",
        py_fn!(
            py,
            generatedynamicconfig(config: config, repo_name: String, shared_path: PyPathBuf)
        ),
    )?;
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
    def load(repopath: Option<PyPathBuf>) -> PyResult<config> {
        let repopath = repopath.as_ref().map(|p| p.as_path());
        let mut cfg = ConfigSet::new();
        cfg.load::<String, String>(repopath, None).map_pyerr(py)?;
        config::create_instance(py, RefCell::new(cfg))
    }

    def reload(&self, repopath: Option<PyPathBuf>, readonly_items: Option<Vec<(String, String)>>) -> PyResult<PyNone> {
        let repopath = repopath.as_ref().map(|p| p.as_path());
        let mut cfg = self.cfg(py).borrow_mut();
        cfg.load(repopath, readonly_items).map_pyerr(py)?;
        Ok(PyNone)
    }

    def ensure_location_supersets(
        &self,
        superset_source: String,
        subset_sources: Vec<String>,
        legacy_list: Vec<(String, String)>
    ) -> PyResult<Vec<(Str, Str, Option<Str>, Option<Str>)>> {
        let legacy_list = HashSet::from_iter(legacy_list.iter().map(|v| (v.0.as_ref(), v.1.as_ref())));

        let results = self.cfg(py).borrow_mut().ensure_location_supersets(superset_source, subset_sources, legacy_list);
        if results.is_empty() {
            return Ok(vec![]);
        }

        let mut output: Vec<(Str, Str, Option<Str>, Option<Str>)> = vec![];
        for ((section, key), value) in results.missing.iter() {
            output.push((section.to_string().into(), key.to_string().into(), None, Some(value.to_string().into())));
        }

        for ((section, key), value) in results.extra.iter() {
            output.push((section.to_string().into(), key.to_string().into(), Some(value.to_string().into()), None));
        }

        for ((section, key), super_value, sub_value) in results.mismatched.iter() {
            output.push((section.to_string().into(), key.to_string().into(), Some(super_value.to_string().into()), Some(sub_value.to_string().into())));
        }

        Ok(output)
    }
});

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

fn applydynamicconfig(
    py: Python,
    config: config,
    repo_name: String,
    shared_path: PyPathBuf,
) -> PyResult<PyNone> {
    let user_name = get_user_name(py, &config);
    let dyn_cfg = Generator::new(repo_name, shared_path.to_path_buf(), user_name)
        .map_pyerr(py)?
        .execute(None)
        .map_pyerr(py)?;
    for section in dyn_cfg.sections() {
        for key in dyn_cfg.keys(section.clone()).iter_mut() {
            if let Some(value) = dyn_cfg.get(section.clone(), key.clone()) {
                config.set(
                    py,
                    section.to_string(),
                    key.to_string(),
                    Some(value.to_string()),
                    "hgrc.dynamic".into(),
                )?;
            }
        }
    }

    Ok(PyNone)
}

fn generatedynamicconfig(
    py: Python,
    config: config,
    repo_name: String,
    shared_path: PyPathBuf,
) -> PyResult<PyNone> {
    let user_name = get_user_name(py, &config);
    generate_dynamicconfig(shared_path.as_path(), repo_name, None, user_name).map_pyerr(py)?;
    Ok(PyNone)
}

fn get_user_name(py: Python, config: &config) -> String {
    config
        .get(py, "ui".to_string(), "username".to_string())
        .unwrap_or(None)
        .and_then(|s| s.to_string(py).map(|s| Some(s.to_string())).unwrap_or(None))
        .unwrap_or_else(|| "".to_string())
}
