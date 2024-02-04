/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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
        Some(m) => match m.source_code() {
            Some(s) => Ok(Some(to_bytes(py, s.as_bytes())?)),
            None => Ok(None),
        },
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

// This is both a finder and a loader.
py_class!(pub class BindingsModuleFinder |py| {
    // If set, modules found from this path will be ignored.
    data home: Option<String>;

    // importlib.machinery.ModuleSpec
    data module_spec: PyObject;

    def __new__(_cls, home: Option<String>) -> PyResult<Self> {
        Self::new(py, home)
    }

    // https://docs.python.org/3/library/importlib.html#importlib.abc.MetaPathFinder

    def find_spec(&self, name: &str, _path: Option<PyObject> = None, _target: Option<PyObject> = None) -> PyResult<Option<PyObject>> {
        match python_modules::find_module(name) {
            None => Ok(None),
            Some(info) => {
                if !info.is_stdlib() {
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
                }
                // ModuleSpec(name, loader, *, origin=None, loader_state=None, is_package=None)
                let loader = self.clone_ref(py).into_object();
                let kwargs = PyDict::new(py);
                kwargs.set_item(py, "is_package", info.is_package())?;
                let spec = self.module_spec(py).call(py, (name, loader), Some(&kwargs))?;
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
        let name = spec.getattr(py, "name")?;
        let name: String = name.extract(py)?;
        let info = match python_modules::find_module(&name) {
            Some(info) => info,
            None => {
                let msg = format!("BindingsModuleFinder cannot load {}", name);
                return Err(PyErr::new::<exc::ImportError, _>(py, msg));
            }
        };
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

    // get_source is part of PEP 302. `linecache` can use it to provide source code.

    def get_source(&self, module_name: &str) -> PyResult<Option<String>> {
        match python_modules::find_module(module_name) {
            None => Err(PyErr::new::<exc::ImportError, _>(py, module_name)),
            Some(m) => Ok(m.source_code().map(|s| s.to_string())),
        }
    }
});

impl BindingsModuleFinder {
    pub fn new(py: Python, home: Option<String>) -> PyResult<Self> {
        bytecode_compatibility_check(py)?;
        // import _frozen_importlib instead of importlib to avoid hitting the filesystem.
        let importlib = py.import("_frozen_importlib")?;
        let module_spec = importlib.get(py, "ModuleSpec")?;
        Self::create_instance(py, home, module_spec)
    }
}
