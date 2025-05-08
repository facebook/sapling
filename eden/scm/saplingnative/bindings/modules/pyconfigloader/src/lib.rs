/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::sync::Arc;

use configloader::config::ConfigSet;
use configloader::config::Options;
use configloader::convert::parse_list;
use configloader::hg::ConfigSetHgExt;
use configloader::hg::OptionsHgExt;
use configloader::hg::RepoInfo;
use configmodel::Config;
use configmodel::Text;
use configmodel::convert::ByteCount;
use configmodel::convert::FromConfigValue;
use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use cpython_ext::error::AnyhowResultExt;
use cpython_ext::error::Result;
use cpython_ext::error::ResultPyErrExt;
use repo_minimal_info::RepoMinimalInfo;

mod impl_into;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "configloader"].join(".");
    let m = PyModule::new(py, &name)?;

    m.add_class::<config>(py)?;

    m.add(py, "parselist", py_fn!(py, parselist(value: String)))?;
    m.add(py, "unset_obj", unset::create_instance(py)?)?;

    impl_into::register(py);

    Ok(m)
}

py_class!(pub class config |py| {
    data cfg: RefCell<ConfigSet>;

    def __new__(_cls) -> PyResult<config> {
        config::create_instance(py, RefCell::new(ConfigSet::new().named("pyconfig")))
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
    ) -> PyResult<Vec<String>> {
        let mut cfg = self.cfg(py).borrow_mut();

        let mut opts = Options::new().source(source).process_hgplain();
        if let Some(sections) = sections {
            opts = opts.filter_sections(sections);
        }
        if let Some(remap) = remap {
            let map = remap.into_iter().collect();
            opts = opts.remap_sections(map);
        }

        let errors = cfg.load_path(path, &opts);
        Ok(errors_to_str_vec(errors))
    }

    def parse(&self, content: String, source: String) -> PyResult<Vec<String>> {
        let mut cfg = self.cfg(py).borrow_mut();
        let opts = source.into();
        let errors = cfg.parse(content, &opts);
        Ok(errors_to_str_vec(errors))
    }

    @property
    def get(&self) -> PyResult<ConfigGetter> {
        let cfg_obj = self.clone_ref(py);
        ConfigGetter::create_instance(py, cfg_obj, Default::default())
    }

    def sources(
        &self, section: &str, name: &str
    ) -> PyResult<Vec<(Option<PyString>, Option<(PyPathBuf, usize, usize, usize)>, PyString)>> {
        // Return [(value, file_source, source)]
        // file_source is a tuple of (file_path, byte_start, byte_end, line)
        let cfg = self.cfg(py).borrow();
        let sources = cfg.get_sources(section, name);
        let mut result = Vec::with_capacity(sources.len());
        for source in sources.as_ref().iter() {
            let value = source.value().as_ref().map(|v| PyString::new(py, v));
            let file = source.location().map(|(path, range)| {
                let line = source.line_number().unwrap_or_default();

                let pypath = if path.as_os_str().is_empty() {
                    PyPathBuf::from(String::from("<builtin>"))
                } else {
                    let path = util::path::strip_unc_prefix(&path);
                    path.try_into().unwrap()
                };
                (pypath, range.start, range.end, line)
            });
            let source = PyString::new(py, source.source());
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

    def sections(&self) -> PyResult<Vec<PyString>> {
        let cfg = self.cfg(py).borrow();
        Ok(cfg.sections().iter().map(|s| PyString::new(py, s)).collect())
    }

    def names(&self, section: &str) -> PyResult<Vec<PyString>> {
        let cfg = self.cfg(py).borrow();
        Ok(cfg.keys(section).iter().map(|s| PyString::new(py, s)).collect())
    }

    def tostring(&self) -> PyResult<String> {
        let cfg = self.cfg(py).borrow();
        Ok(cfg.to_string())
    }

    @staticmethod
    def load(repopath: Option<PyPathBuf>) -> PyResult<Self> {
        let info = path_to_info(py, repopath)?;
        let info = match info {
            Some(ref info) => RepoInfo::Disk(info),
            None => RepoInfo::NoRepo,
        };
        let mut cfg = ConfigSet::new();
        cfg.load(info, Default::default()).map_pyerr(py)?;
        Self::create_instance(py, RefCell::new(cfg))
    }

    def reload(
        &self,
        repopath: Option<PyPathBuf>,
    ) -> PyResult<PyNone> {
        let info = path_to_info(py, repopath)?;
        let info = match info {
            Some(ref info) => RepoInfo::Disk(info),
            None => RepoInfo::NoRepo,
        };
        let mut cfg = self.cfg(py).borrow_mut();
        cfg.load(info, Default::default()).map_pyerr(py)?;
        Ok(PyNone)
    }

    def files(&self) -> PyResult<Vec<PyPathBuf>> {
        self.cfg(py).borrow().files().iter().map(|(p, _)| p.as_path().try_into()).collect::<Result<Vec<PyPathBuf>>>().map_pyerr(py)
    }
});

#[derive(Default, Clone, Copy)]
struct TypeDef {
    convert: Option<fn(Python, &str) -> anyhow::Result<PyObject>>,
    /// If set, it is not nullable.
    default: Option<fn(Python) -> PyObject>,
}

impl TypeDef {
    fn new(convert: fn(Python, &str) -> anyhow::Result<PyObject>) -> Self {
        Self {
            convert: Some(convert),
            default: None,
        }
    }

    fn with_default(mut self, default: fn(Python) -> PyObject) -> Self {
        self.default = Some(default);
        self
    }
}

py_class!(class unset |_py| {});

/// Unlike `Option`, distinguish between "set to None" and "not set".
/// Use-case: `get.as_bool(x, y, None)` should return `None` instead of `False`
/// for "not set" configs.
enum PyOption {
    Absent,
    Present(PyObject),
}

impl<'s> FromPyObject<'s> for PyOption {
    fn extract(py: Python, obj: &'s PyObject) -> PyResult<Self> {
        if obj.cast_as::<unset>(py).is_ok() {
            Ok(PyOption::Absent)
        } else {
            Ok(PyOption::Present(obj.clone_ref(py)))
        }
    }
}

py_class!(class ConfigGetter |py| {
    data cfg_obj: config;
    data type_def: TypeDef;

    def __call__(&self, section: &str, name: &str, default: PyOption = PyOption::Absent) -> PyResult<Option<PyObject>> {
        let cfg = self.cfg_obj(py).cfg(py);
        let cfg = cfg.borrow();
        let type_def = self.type_def(py);
        let value: Option<Text> = cfg.get(section, name);
        let convert = type_def.convert;
        let implicit_default = type_def.default;
        let value: Option<PyObject> = match (value, convert, implicit_default, default) {
            (Some(text), None, _, _) => Some(PyString::new(py, &text).into_object()),
            (Some(text), Some(convert), _, _) => Some(convert(py, &text).map_err(|e| PyErr::new::<exc::ValueError, _>(py, format!("invalid config {section}.{name}={text}: {e}")))?),
            (None, _, None, PyOption::Absent) => None,
            (None, _, Some(default_func), PyOption::Absent) => Some(default_func(py)),
            (None, None, _, PyOption::Present(v)) => Some(v),
            (None, Some(convert), _, PyOption::Present(v)) => match v.extract::<String>(py) {
                // NOTE: 'default' could be before-convert (ex. str), or after-convert (not str).
                Ok(s) => Some(convert(py, &s).map_err(|e| PyErr::new::<exc::ValueError, _>(py, format!("invalid default config {section}.{name}={s}: {e}")))?),
                Err(_) => Some(v),
            },
        };
        Ok(value)
    }

    /// Read as nullable str (default if no `as_*` is called).
    @property
    def as_str(&self) -> PyResult<Self> {
        let cfg_obj = self.cfg_obj(py).clone_ref(py);
        Self::create_instance(py, cfg_obj, Default::default())
    }

    /// Read as nullable integer.
    @property
    def as_int(&self) -> PyResult<Self> {
        let type_def = TypeDef::new(|py, text| Ok(i64::try_from_str(text)?.to_py_object(py).into_object()));
        self.with_type_def(py, type_def)
    }

    /// Read as bool. Missing values are treated as "false".
    @property
    def as_bool(&self) -> PyResult<Self> {
        let type_def = TypeDef::new(|py, text| Ok(bool::try_from_str(text)?.to_py_object(py).into_object()));
        let type_def = type_def.with_default(|py| py.False().into_object());
        self.with_type_def(py, type_def)
    }

    /// Read as nullable bool.
    @property
    def as_optional_bool(&self) -> PyResult<Self> {
        let type_def = TypeDef::new(|py, text| Ok(bool::try_from_str(text)?.to_py_object(py).into_object()));
        self.with_type_def(py, type_def)
    }

    /// Read as list. Missing values are treated as an empty list.
    @property
    def as_list(&self) -> PyResult<Self> {
        let type_def = TypeDef::new(|py, text| Ok(Vec::<String>::try_from_str(text)?.to_py_object(py).into_object()));
        let type_def = type_def.with_default(|py| PyList::new(py, &[]).into_object());
        self.with_type_def(py, type_def)
    }

    /// Read as byte count (ex. "1gb"). Missing values are treated as 0.
    @property
    def as_byte_count(&self) -> PyResult<Self> {
        let type_def = TypeDef::new(|py, text| Ok(ByteCount::try_from_str(text)?.value().to_py_object(py).into_object()));
        let type_def = type_def.with_default(|py| 0i32.to_py_object(py).into_object());
        self.with_type_def(py, type_def)
    }

    /// Read as nullable (compiled) regex.
    @property
    def as_regex(&self) -> PyResult<Self> {
        let type_def = TypeDef::new(|py, text| {
            let native = pyregex::Regex::try_from_str(text)?;
            Ok(pyregex::StringPattern::from_native(py, native).into_anyhow_result()?.into_object())
        });
        self.with_type_def(py, type_def)
    }

    /// Read as nullable (compiled) matcher.
    /// Config value is a list of gitignore rules. Respect `!` and order matters.
    @property
    def as_matcher(&self) -> PyResult<Self> {
        let type_def = TypeDef::new(|py, text| {
            let native = pypathmatcher::TreeMatcher::try_from_str(text)?;
            Ok(pypathmatcher::treematcher::from_native(py, native).into_anyhow_result()?.into_object())
        });
        self.with_type_def(py, type_def)
    }
});

impl ConfigGetter {
    fn with_type_def(&self, py: Python, def: TypeDef) -> PyResult<Self> {
        let cfg_obj = self.cfg_obj(py).clone_ref(py);
        Self::create_instance(py, cfg_obj, def)
    }
}

fn path_to_info(py: Python, path: Option<PyPathBuf>) -> PyResult<Option<RepoMinimalInfo>> {
    // Ideally the callsite can provide `info` directly.
    let info = match path {
        None => None,
        Some(p) => Some(RepoMinimalInfo::from_repo_root(p.to_path_buf()).map_pyerr(py)?),
    };
    Ok(info)
}

impl config {
    pub fn get_cfg(&self, py: Python) -> ConfigSet {
        self.cfg(py).clone().into_inner()
    }

    pub(crate) fn get_config_trait(&self, py: Python) -> Arc<dyn Config> {
        Arc::new(self.get_cfg(py))
    }

    pub(crate) fn get_thread_safe_config_trait(&self, py: Python) -> Arc<dyn Config + Send + Sync> {
        Arc::new(self.get_cfg(py))
    }

    pub fn from_dyn_config(py: Python, config: Arc<dyn Config>) -> PyResult<Self> {
        let mut cfg = ConfigSet::new();
        cfg.secondary(config);
        Self::create_instance(py, RefCell::new(cfg))
    }
}

fn parselist(py: Python, value: String) -> PyResult<Vec<PyString>> {
    Ok(parse_list(value)
        .iter()
        .map(|v| PyString::new(py, v))
        .collect())
}

fn errors_to_str_vec(errors: Vec<configloader::error::Error>) -> Vec<String> {
    errors.into_iter().map(|err| format!("{}", err)).collect()
}
