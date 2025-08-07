/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use blake2::Blake2s256;
use blake2::Digest;
use cpython::exc::ImportError;
use cpython::*;
use cpython_ext::AnyhowResultExt;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
pub use erased_serde;
use erased_serde::Serialize;
use repo::Repo;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "hook"].join(".");
    let m = PyModule::new(py, &name)?;

    m.add(
        py,
        "get_callable",
        py_fn!(py, get_callable(spec: &str, repo_root: Option<&PyPath>, hook_name: &str)),
    )?;
    m.add(
        py,
        "load_path",
        py_fn!(py, load_path(path: PyPathBuf, module_name: &str)),
    )?;
    m.add(
        py,
        "load_source",
        py_fn!(py, load_source(source: &str, module_name: &str)),
    )?;
    m.add(
        py,
        "run_python_hook",
        py_fn!(py, run_python_hook_py(repo: Option<pyrepo::repo>, spec: &str, hook_name: &str, kwargs: Option<PyDict> = None)),
    )?;

    Ok(m)
}

/// Run a hook defined by a string. Returns its return code.
/// Intended to be called from the Rust side, to support Python hooks.
///
/// Does NOT update the "blocked" time interval. The callsite might
/// want to do that instead.
///
/// `spec` might look like:
/// - sapling.hooks.foobar (sapling.hooks module, "foobar" function)
/// - path/to/hook.py:foobar (file relative to repo root, "foobar" function)
pub fn run_python_hook(
    repo: Option<&Repo>,
    spec: &str,
    hook_name: &str,
    kwargs: Option<&dyn Serialize>,
) -> anyhow::Result<i8> {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let kwargs = match kwargs {
        Some(kwargs) => {
            let kwargs = cpython_ext::ser::to_object(py, &kwargs).into_anyhow_result()?;
            Some(kwargs.extract::<PyDict>(py).into_anyhow_result()?)
        }
        None => None,
    };
    let py_repo = match repo {
        Some(repo) => Some(pyrepo::repo::from_native(py, repo.clone()).into_anyhow_result()?),
        None => None,
    };
    run_python_hook_py(py, py_repo, spec, hook_name, kwargs).into_anyhow_result()
}

/// `run_python_hook`, but intended to be used as a binding function.
fn run_python_hook_py(
    py: Python,
    repo: Option<pyrepo::repo>,
    spec: &str,
    hook_name: &str,
    kwargs: Option<PyDict>,
) -> PyResult<i8> {
    // Set "repo" as a kwarg for the hook function to use.
    let kwargs = match kwargs {
        None => PyDict::new(py),
        Some(kwargs) => kwargs,
    };
    let repo_root = match repo.as_ref() {
        Some(repo) => Some(repo.path(py)?),
        None => None,
    };
    kwargs.set_item(py, "repo", repo.map(|repo| repo.clone_ref(py)))?;
    kwargs.set_item(py, "io", pyio::IO::main(py)?)?;
    let callable = get_callable(py, spec, repo_root.as_deref(), hook_name)?;
    let result = callable.call(py, NoArgs, Some(&kwargs))?;
    Ok(result.extract::<i8>(py).unwrap_or(0))
}

// Similar to sapling.hook._getpyhook.
//
// `spec` can be `file_path:func_name`, or `module_name.func_name`,
// or `base64:...:func_name`.
fn get_callable(
    py: Python,
    spec: &str,
    repo_root: Option<&PyPath>,
    hook_name: &str,
) -> PyResult<PyObject> {
    match spec.rsplit_once(':') {
        Some((left, func_name)) => {
            let module_name = format!("slhook_{}", hook_name);
            let module = match left.strip_prefix("base64:") {
                Some(source_base64) => {
                    let source = BASE64.decode(source_base64).map_pyerr(py)?;
                    let source = String::from_utf8(source).map_pyerr(py)?;
                    load_source(py, &source, &module_name)
                }
                None => {
                    let mut path = util::path::expand_path(left);
                    if let Some(repo_root) = repo_root {
                        path = repo_root.as_path().join(path);
                    }
                    let path = PyPathBuf::try_from(path).map_pyerr(py)?;
                    load_path(py, path, &module_name)
                }
            }?;
            module.getattr(py, func_name)
        }
        None => {
            // See `sapling.hook._pythonhook`.
            // This is usually useful for builtin hooks (ex. sapling.hooks.x).
            let (module_name, func_name) = match spec.rsplit_once('.') {
                Some(v) => v,
                None => {
                    return Err(PyErr::new::<ImportError, _>(
                        py,
                        format!("invalid python hook: {}", spec),
                    ));
                }
            };
            let module = py.import(module_name)?;
            module.get(py, func_name)
        }
    }
}

/// Load a Python standalone module from the given path.
///
/// Calling `load_path` again with the same `path` and `module_name` will reuse
/// the existing `sys.modules[module_name]`, if the file content at `path` does
/// not change.
///
/// Note: this "is changed" check only applies to `path`, not other modules
/// imported by the module. If you want to apply the check for other modules
/// that might change at runtime, you can use this `load_path` API at runtime.
/// For example:
///
/// ```python,ignore
///     def my_function():
///         # do not use `import my_module`
///         my_module = load_path("my_module.py", "my_module")
///         ...
/// ```
fn load_path(py: Python, path: PyPathBuf, module_name: &str) -> PyResult<PyObject> {
    let path: PathBuf = path.to_path_buf();

    let source = match std::fs::read_to_string(&path) {
        Ok(v) => v,
        Err(e) => {
            return Err(PyErr::new::<exc::ImportError, _>(
                py,
                format!("{}: {}", path.display(), e),
            ));
        }
    };

    let module = load_source(py, &source, module_name)?;
    if let Ok(path) = PyPathBuf::try_from(path) {
        module.setattr(py, "__file__", path)?;
    }

    Ok(module)
}

/// Construct a Python standalone module from the given source code.
/// Reuse `sys.modules[module_name]` if `__source_hash__` does not change.
fn load_source(py: Python, source: &str, module_name: &str) -> PyResult<PyObject> {
    let new_hash = {
        let mut hasher = Blake2s256::new();
        hasher.update(source.as_bytes());
        hasher.finalize()
    };

    let sys_modules = py.import("sys")?.get(py, "modules")?;
    if let Ok(module) = sys_modules.get_item(py, module_name) {
        if let Ok(old_hash) = module.getattr(py, "__source_hash__") {
            if let Ok(old_hash) = old_hash.extract::<PyBytes>(py) {
                if old_hash.data(py) == new_hash.as_slice() {
                    tracing::debug!(target: "pyhook::load_source", module_name, "reused module");
                    return Ok(module);
                }
            }
        }
    }

    tracing::debug!(target: "pyhook::load_source", module_name, "imported module");

    let module = create_module_from_source_code(py, module_name, &source)?;
    module.add(py, "__source_hash__", PyBytes::new(py, new_hash.as_slice()))?;

    // sys.modules[module_name] = module
    sys_modules.set_item(py, module_name, module.clone_ref(py))?;

    Ok(module.into_object())
}

/// Create a Python module from source code.
/// Bypasses importlib to reduce overhead.
fn create_module_from_source_code(
    py: Python,
    module_name: &str,
    source: &str,
) -> PyResult<PyModule> {
    // Note: `importlib` approach (D72691047 v1):
    //
    //     // spec = importlib.util.spec_from_file_location(module_name, path)
    //     // Note: prefer `_frozen_*` to `importlib` to minimize import overhead.
    //     let spec_from_file_location = py
    //         .import("_frozen_importlib_external")?
    //         .get(py, "spec_from_file_location")?;
    //     let spec = spec_from_file_location.call(py, (&module_name, path), None)?;
    //     // module = importlib.util.module_from_spec(spec)
    //     let module_from_spec = py
    //         .import("_frozen_importlib")?
    //         .get(py, "module_from_spec")?;
    //     let module = module_from_spec.call(py, (&spec,), None)?;
    //     // sys.modules[module_name] = module
    //     let sys_modules = py.import("sys")?.get(py, "modules")?;
    //     sys_modules.set_item(py, module_name, module.clone_ref(py))?;
    //     // spec.loader.exec_module(module)
    //     let loader = spec.getattr(py, "loader")?;
    //     loader.call_method(py, "exec_module", (&module,), None)?;
    //
    // Took 1.2ms to load a simple module even with its `__pycache__` populated:
    //
    //     In [1]: %time m=b.hook.load_path('/tmp/a.py', 'a.py')
    //     CPU times: user 1.23 ms, sys: 0 ns, total: 1.23 ms
    //     Wall time: 418 µs
    //
    // Without `importlib` (this function), 1000x faster:
    //
    //     In [1]: %time m=b.hook.load_path('/tmp/a.py', 'a.py')
    //     CPU times: user 117 µs, sys: 12 µs, total: 129 µs
    //     Wall time: 134 µs
    let module = PyModule::new(py, module_name)?;
    let globals = module.dict(py);
    py.run(source, Some(&globals), None)?;
    Ok(module)
}
