// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![allow(non_camel_case_types)]

use std::{cell::RefCell, collections::HashMap};

use cpython::exc::UnicodeDecodeError;
use cpython::*;

use ::configparser::{
    config::{ConfigSet, Options},
    hg::{parse_list, ConfigSetHgExt, OptionsHgExt},
};
use encoding::{local_bytes_to_path, path_to_local_bytes};

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "configparser"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<config>(py)?;
    m.add(py, "parselist", py_fn!(py, parselist(value: PyBytes)))?;
    Ok(m)
}

py_class!(class config |py| {
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
        path: &PyBytes,
        source: &PyBytes,
        sections: Option<Vec<PyBytes>>,
        remap: Option<Vec<(PyBytes, PyBytes)>>,
        readonly_items: Option<Vec<(PyBytes, PyBytes)>>
    ) -> PyResult<Vec<PyBytes>> {
        let path = local_bytes_to_path(path.data(py)).map_err(|_| encoding_error(py, path))?;
        let mut cfg = self.cfg(py).borrow_mut();

        let mut opts = Options::new().source(source.data(py)).process_hgplain();
        if let Some(sections) = sections {
            let sections = sections.into_iter().map(|section| section.data(py).to_vec()).collect();
            opts = opts.whitelist_sections(sections);
        }
        if let Some(remap) = remap {
            let mut map = HashMap::new();
            for (key, value) in remap {
                map.insert(key.data(py).to_vec(), value.data(py).to_vec());
            }
            opts = opts.remap_sections(map);
        }
        if let Some(readonly_items) = readonly_items {
            let items: Vec<(Vec<u8>, Vec<u8>)> = readonly_items.iter()
                .map(|&(ref section, ref name)| {
                    (section.data(py).to_vec(), name.data(py).to_vec())
                }).collect();
            opts = opts.readonly_items(items);
        }

        let errors = cfg.load_path(path, &opts);
        Ok(errors_to_pybytes_vec(py, errors))
    }

    def parse(&self, content: &PyBytes, source: &PyBytes) -> PyResult<Vec<PyBytes>> {
        let mut cfg = self.cfg(py).borrow_mut();
        let opts = source.data(py).into();
        let errors = cfg.parse(content.data(py), &opts);
        Ok(errors_to_pybytes_vec(py, errors))
    }

    def get(&self, section: &PyBytes, name: &PyBytes) -> PyResult<Option<PyBytes>> {
        let cfg = self.cfg(py).borrow();
        Ok(cfg.get(section.data(py), name.data(py)).map(|bytes| PyBytes::new(py, &bytes)))
    }

    def sources(
        &self, section: &PyBytes, name: &PyBytes
    ) -> PyResult<Vec<(Option<PyBytes>, Option<(PyBytes, usize, usize, usize)>, PyBytes)>> {
        // Return [(value, file_source, source)]
        // file_source is a tuple of (file_path, byte_start, byte_end, line)
        let cfg = self.cfg(py).borrow();
        let sources = cfg.get_sources(section.data(py), name.data(py));
        let mut result = Vec::with_capacity(sources.len());
        for source in sources {
            let value = source.value().clone().map(|bytes| PyBytes::new(py, &bytes));
            let file = source.location().map(|(path, range)| {
                let bytes = path_to_local_bytes(&path).unwrap();
                // Calculate the line number - count "\n" till range.start
                let file = source.file_content().unwrap();
                let line = 1 + file.slice(0, range.start).iter().filter(|ch| **ch == b'\n').count();
                let pypath = if cfg!(windows) {
                    // path.caonicalize() used internally by configparser
                    // adds "\\?\" prefix on Windows.
                    // It's unfriendly to users. Strip them.
                    // Related: https://github.com/rust-lang/rust/issues/42869
                    PyBytes::new(py, &bytes[4..bytes.len()])
                } else {
                    PyBytes::new(py, &bytes)
                };
                (pypath, range.start, range.end, line)
            });
            let source = PyBytes::new(py, source.source());
            result.push((value, file, source));
        }
        Ok(result)
    }

    def set(
        &self, section: &PyBytes, name: &PyBytes, value: Option<&PyBytes>, source: &PyBytes
    ) -> PyResult<PyObject> {
        let mut cfg = self.cfg(py).borrow_mut();
        let opts = source.data(py).into();
        cfg.set(section.data(py), name.data(py), value.map(|v| v.data(py)), &opts);
        Ok(py.None())
    }

    def sections(&self) -> PyResult<Vec<PyBytes>> {
        let cfg = self.cfg(py).borrow();
        let sections: Vec<PyBytes> = cfg.sections()
            .iter().map(|bytes| PyBytes::new(py, bytes)).collect();
        Ok(sections)
    }

    def names(&self, section: &PyBytes) -> PyResult<Vec<PyBytes>> {
        let cfg = self.cfg(py).borrow();
        let keys: Vec<PyBytes> = cfg.keys(section.data(py))
            .iter().map(|bytes| PyBytes::new(py, bytes)).collect();
        Ok(keys)
    }

    @staticmethod
    def load(datapath: PyBytes) -> PyResult<(config, Vec<PyBytes>)> {
        let datapath = local_bytes_to_path(datapath.data(py))
            .map_err(|_| encoding_error(py, &datapath))?;
        let mut cfg = ConfigSet::new();
        let mut errors = Vec::new();
        errors.append(&mut cfg.load_system(datapath));
        errors.append(&mut cfg.load_user());
        let errors = errors_to_pybytes_vec(py, errors);
        config::create_instance(py, RefCell::new(cfg)).map(|cfg| (cfg, errors))
    }
});

fn parselist(py: Python, value: PyBytes) -> PyResult<Vec<PyBytes>> {
    let value = value.data(py);
    let list = parse_list(value);
    Ok(list.into_iter().map(|v| PyBytes::new(py, &v)).collect())
}

fn encoding_error(py: Python, input: &PyBytes) -> PyErr {
    use std::ffi::CStr;
    let utf8 = CStr::from_bytes_with_nul(b"utf8\0").unwrap();
    let reason = CStr::from_bytes_with_nul(b"invalid encoding\0").unwrap();
    let input = input.data(py);
    let err = UnicodeDecodeError::new(py, utf8, input, 0..input.len(), reason).unwrap();
    PyErr::from_instance(py, err)
}

fn errors_to_pybytes_vec(py: Python, errors: Vec<configparser::error::Error>) -> Vec<PyBytes> {
    errors
        .iter()
        .map(|err| PyBytes::new(py, format!("{}", err).as_bytes()))
        .collect()
}
