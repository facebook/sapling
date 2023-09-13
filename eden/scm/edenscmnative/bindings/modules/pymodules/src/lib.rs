/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::Cell;
use std::cell::RefCell;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use cpython::_detail::ffi;
use cpython::*;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "modules"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(py, "get_bytecode", py_fn!(py, get_bytecode(name: &str)))?;
    m.add(py, "get_source", py_fn!(py, get_source(name: &str)))?;
    m.add(py, "list", py_fn!(py, list()))?;
    m.add_class::<BindingsModuleFinder>(py)?;
    Ok(m)
}

fn bytecode_compatibility_check(py: Python) -> PyResult<()> {
    static CHECKED: AtomicBool = AtomicBool::new(false);
    if !CHECKED.swap(true, Ordering::AcqRel) {
        let sys = py.import("sys")?;
        let version_info = sys.get(py, "version_info")?;
        let major: usize = version_info.getattr(py, "major")?.extract(py)?;
        let minor: usize = version_info.getattr(py, "minor")?.extract(py)?;
        let compiled = (python_modules::VERSION_MAJOR, python_modules::VERSION_MINOR);
        let current = (major, minor);
        if compiled != current {
            // This is a serious fatal error.
            panic!(
                "Compiled bytecode version ({:?}) does not match the current interpreter ({:?})",
                compiled, current
            );
        }
    }
    Ok(())
}

fn get_bytecode(py: Python, name: &str) -> PyResult<Option<::pybytes::Bytes>> {
    match python_modules::find_module(name) {
        None => Ok(None),
        Some(m) => Ok(Some(to_bytes(py, m.byte_code())?)),
    }
}

fn get_source(py: Python, name: &str) -> PyResult<Option<::pybytes::Bytes>> {
    match python_modules::find_module(name) {
        None => Ok(None),
        Some(m) => Ok(Some(to_bytes(py, m.source_code().as_bytes())?)),
    }
}

fn list(_py: Python) -> PyResult<Vec<&'static str>> {
    Ok(python_modules::list_modules())
}

/// Converts to `pybytes::Bytes` without copying the slice.
fn to_bytes(py: Python, slice: &'static [u8]) -> PyResult<::pybytes::Bytes> {
    let bytes = minibytes::Bytes::from_static(slice);
    pybytes::Bytes::from_bytes(py, bytes)
}

py_class!(pub class BindingsModuleFinder |py| {
    // If set, modules found from this path will be ignored.
    data home: Option<String>;

    def __new__(_cls, home: Option<String>) -> PyResult<Self> {
        Self::new(py, home)
    }

    // https://docs.python.org/3/library/importlib.html#importlib.abc.MetaPathFinder

    def find_spec(&self, name: &str, _path: Option<PyObject> = None, _target: Option<PyObject> = None) -> PyResult<Option<ModuleSpec>> {
        match python_modules::find_module(name) {
            None => Ok(None),
            Some(info) => {
                let home = self.home(py);
                if let Some(home) = home {
                    let path = if info.is_package() {
                        format!("{}/{}/__init__.py", home, name.replace('.', "/"))
                    } else {
                        format!("{}/{}.py", home, name.replace('.', "/"))
                    };
                    if Path::new(&path).exists() {
                        // Fallback to other finders.
                        return Ok(None);
                    }
                }
                let loader = self.clone_ref(py).into_object();
                let spec = ModuleSpec::create_instance(
                    py,
                    info,
                    Cell::new(false),
                    RefCell::new(Some(loader)),
                    RefCell::new(None),
                )?;
                Ok(Some(spec))
            }
        }
    }

    def invalidate_caches(&self) -> PyResult<Option<PyObject>> {
        Ok(None)
    }

    // https://docs.python.org/3/library/importlib.html#importlib.abc.Loader

    def create_module(&self, _spec: Option<PyObject> = None) -> PyResult<Option<PyModule>> {
        Ok(None) // use default module creation
    }

    def exec_module(&self, module: PyModule) -> PyResult<PyObject> {
        let spec = module.get(py, "__spec__")?;
        let spec: ModuleSpec = spec.cast_into(py).map_err(|e| {
            PyErr::new::<exc::TypeError, _>(py, format!("BindingsModuleLoader cannot load other modules: {:?}", e))
        })?;
        let info = spec.info(py);
        let bytecode = info.byte_code();
        let c_name = info.c_name();
        let obj = unsafe {
            let code = ffi::PyMarshal_ReadObjectFromString(bytecode.as_ptr() as _, bytecode.len() as _);
            if code.is_null() {
                return Err(PyErr::fetch(py));
            }
            let m = ffi::PyImport_ExecCodeModule(c_name.as_ptr() as _, code);
            ffi::Py_XDECREF(code);
            if m.is_null() {
                return Err(PyErr::fetch(py));
            }
            PyObject::from_owned_ptr(py, m)
        };
        Ok(obj)
    }

    // `get_code` is not part of modern importlib loader spec. It is "optional" by PEP 302.
    // However, runpy.py from stdlib requires it.

    def get_code(&self, module_name: &str) -> PyResult<Option<PyObject>> {
        match python_modules::find_module(module_name) {
            None => Ok(None),
            Some(info) => unsafe {
                let bytecode = info.byte_code();
                let code = ffi::PyMarshal_ReadObjectFromString(bytecode.as_ptr() as _, bytecode.len() as _);
                if code.is_null() {
                    return Err(PyErr::fetch(py));
                }
                Ok(Some(PyObject::from_owned_ptr(py, code)))
            }
        }
    }

});

impl BindingsModuleFinder {
    pub fn new(py: Python, home: Option<String>) -> PyResult<Self> {
        bytecode_compatibility_check(py)?;
        Self::create_instance(py, home)
    }
}

// Duck-typed `importlib.machinery.ModuleSpec` to avoid importing `importlib`.
// https://docs.python.org/3/library/importlib.html#importlib.machinery.ModuleSpec
py_class!(pub class ModuleSpec |py| {
    data info: python_modules::ModuleInfo;
    data initializing: Cell<bool>;
    data loader_override: RefCell<Option<PyObject>>;
    data loader_state_override: RefCell<Option<PyObject>>;

    @property
    def name(&self) -> PyResult<&'static str> {
        Ok(self.info(py).name())
    }

    @property
    def loader(&self) -> PyResult<Option<PyObject>> {
        Ok(self.loader_override(py).borrow().clone_ref(py))
    }

    // Used by hgdemandimport
    @loader.setter
    def set_loader(&self, obj: Option<PyObject>) -> PyResult<()> {
        let mut loader = self.loader_override(py).borrow_mut();
        *loader = obj;
        Ok(())
    }

    @property
    def origin(&self) -> PyResult<Option<String>> {
        Ok(None)
    }

    @property
    def submodule_search_locations(&self) -> PyResult<Option<Vec<String>>> {
        let info = self.info(py);
        if info.is_package() {
            Ok(Some(Vec::new()))
        } else {
            Ok(None)
        }
    }

    @property
    def loader_state(&self) -> PyResult<Option<PyObject>> {
        Ok(self.loader_state_override(py).borrow().clone_ref(py))
    }

    // used by importlib.util
    @loader_state.setter
    def set_loader_state(&self, obj: Option<PyObject>) -> PyResult<()> {
        let mut loader_state = self.loader_state_override(py).borrow_mut();
        *loader_state = obj;
        Ok(())
    }

    @property
    def cached(&self) -> PyResult<Option<PyObject>> {
        Ok(None)
    }

    @property
    def parent(&self) -> PyResult<&'static str> {
        let info = self.info(py);
        let name = info.name();
        if info.is_package() {
            Ok(name)
        } else {
            match name.rsplit_once('.') {
                None => Ok(""),
                Some((parent, _)) => Ok(parent),
            }
        }
    }

    @property
    def has_location(&self) -> PyResult<bool> {
        Ok(false)
    }

    @property
    def _initializing(&self) -> PyResult<bool> {
        Ok(self.initializing(py).get())
    }

    @_initializing.setter
    def set_initializing(&self, value: Option<bool>) -> PyResult<()> {
        let value = value.unwrap_or_default();
        self.initializing(py).set(value);
        Ok(())
    }

});
