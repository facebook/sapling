/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;
use std::env;
use std::path::PathBuf;

use clidispatch::io::IO;
use configloader::config::ConfigSet;
use cpython::*;
use cpython_ext::format_py_error;
use cpython_ext::wrap_pyio;
use cpython_ext::Bytes;
use cpython_ext::ResultPyErrExt;
use cpython_ext::Str;
use cpython_ext::WrappedIO;
use tracing::debug_span;
use tracing::info_span;

use crate::commands;
use crate::python::py_finalize;
use crate::python::py_init_threads;
use crate::python::py_initialize;
use crate::python::py_is_initialized;
use crate::python::py_main;
use crate::python::py_set_argv;
use crate::python::py_set_program_name;

const HGPYENTRYPOINT_MOD: &str = "edenscm";
pub struct HgPython {
    py_initialized_by_us: bool,
}

impl HgPython {
    pub fn new(args: &[String]) -> HgPython {
        let py_initialized_by_us = !py_is_initialized();
        if py_initialized_by_us {
            Self::setup_python(args);
        }
        HgPython {
            py_initialized_by_us,
        }
    }

    fn setup_python(args: &[String]) {
        let span = info_span!("Initialize Python");
        let _guard = span.enter();
        let args = Self::prepare_args(args);
        let executable_name = &args[0];
        py_set_program_name(executable_name);
        py_initialize();
        py_set_argv(&args);
        py_init_threads();

        let gil = Python::acquire_gil();
        let py = gil.python();

        // Putting the module in sys.modules makes it importable.
        let sys = py.import("sys").unwrap();
        HgPython::update_path(py, &sys);

        // If this fails, it's a fatal error.
        let name = "bindings";
        let bindings_module = PyModule::new(py, &name).unwrap();
        prepare_builtin_modules(py, &bindings_module).unwrap();
        let sys_modules = PyDict::extract(py, &sys.get(py, "modules").unwrap()).unwrap();
        sys_modules.set_item(py, name, bindings_module).unwrap();
    }

    fn update_path(py: Python, sys: &PyModule) {
        // In homebrew and other environments, the python modules may be installed isolated
        // alongside the binary. Let's setup the PATH so we discover those python modules.
        // An example layout:
        //   $PREFIX/usr/local/bin/hg
        //   $PREFIX/usr/local/lib/python3.8/site-packages/edenscmnative
        //   $PREFIX/usr/local/lib/python3.8/site-packages/edenscmdeps3.zip
        //   $PREFIX/usr/local/lib/python3.8/site-packages/edenscm
        let py_version: (i32, i32, i32, String, i32) =
            sys.get(py, "version_info").unwrap().extract(py).unwrap();

        let path_for_prefix = |prefix: &str| -> String {
            let rel_path = PathBuf::from(format!(
                "{}/python{}.{}/site-packages",
                prefix, py_version.0, py_version.1
            ));
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join(rel_path)
                .into_os_string()
                .into_string()
                .unwrap()
        };
        let py_path: PyList = sys.get(py, "path").unwrap().extract(py).unwrap();
        py_path.append(
            py,
            PyUnicode::new(py, &path_for_prefix("lib")).into_object(),
        );
        py_path.append(
            py,
            PyUnicode::new(py, &path_for_prefix("lib64")).into_object(),
        );
    }

    fn prepare_args(args: &[String]) -> Vec<String> {
        // Replace args[0] with the absolute current_exe path. This workarounds
        // an issue in libpython sys.path handling.
        //
        // More context: Usually, argv[0] is either:
        // - a relative path to the executable, like "hg", or "./hg". It can be
        //   translated to an absolute path using the PATH environment variable
        //   and the current workdir.
        // - an absolute path to the executable, like "/bin/hg".
        //
        // When running as local build, we expect libpython to add the
        // "executable path" to sys.path. However, libpython seems pretty dumb
        // if argv[0] is a relative path, and it's not in the current workdir
        // (in other words, libpython seems to ignore PATH). Therefore, give
        // it some hint by passing the absolute path resolved by the Rust stdlib.
        Some(
            env::current_exe()
                .unwrap()
                .into_os_string()
                .into_string()
                .unwrap(),
        )
        .into_iter()
        .chain(args.iter().skip(1).cloned())
        .collect()
    }

    fn run_hg_py(
        &self,
        py: Python<'_>,
        args: Vec<String>,
        io: &clidispatch::io::IO,
        config: &ConfigSet,
    ) -> PyResult<()> {
        let entry_point_mod =
            info_span!("import edenscm").in_scope(|| py.import(HGPYENTRYPOINT_MOD))?;
        let call_args = {
            let fin = io.with_input(|i| read_to_py_object(py, i));
            let fout = io.with_output(|o| write_to_py_object(py, o));
            let ferr = io.with_error(|e| match e {
                None => fout.clone_ref(py),
                Some(error) => write_to_py_object(py, error),
            });
            let args: Vec<Str> = args.into_iter().map(Str::from).collect();
            let config =
                pyconfigloader::config::create_instance(py, RefCell::new(config.clone())).unwrap();
            (args, fin, fout, ferr, config).to_py_object(py)
        };
        entry_point_mod.call(py, "run", call_args, None)?;
        Ok(())
    }

    /// Run an hg command defined in Python.
    pub fn run_hg(&self, args: Vec<String>, io: &clidispatch::io::IO, config: &ConfigSet) -> i32 {
        let gil = Python::acquire_gil();
        let py = gil.python();
        match self.run_hg_py(py, args, io, config) {
            // The code below considers the following exit scenarios:
            // - `PyResult` is `Ok`. This means that the Python code returned
            //    successfully, without calling `sys.exit` or raising an
            //    uncaught exception
            // - `PyResult` is a `PyErr(SystemExit)`. This means that the Python
            //    code called `sys.exit`.
            //    - The expected case is that the `SystemExit` instance contains
            //      a `code` attribute, which is the desired exit code.
            //    - If it does not, we fail hard with code 255.
            // - `PyResult` is a `PyErr(UncaughtException)`. Something went wrong.
            //    Python's behavior in this case is to print a traceback and to
            //    return 1.
            Err(mut err) => {
                if let Ok(system_exit) = err.instance(py).extract::<exc::SystemExit>(py) {
                    match system_exit.as_object().getattr(py, "code") {
                        Ok(code) => code.extract::<i32>(py).unwrap(),
                        Err(_) => 255,
                    }
                } else {
                    let message =
                        format_py_error(py, &err).unwrap_or("unknown python exception".to_string());
                    let _ = io.write_err(&message);
                    1
                }
            }
            Ok(()) => 0,
        }
    }

    /// Setup ad-hoc tracing with `pattern` about modules.
    /// See `edenscm/traceimport.py` for details.
    ///
    /// Call this before `run_python`, or `run_hg`.
    ///
    /// This is merely to provide convenience.  The user can achieve the same
    /// effect via `run_python`, then import the modules and calling methods
    /// manually.
    pub fn setup_tracing(&mut self, pattern: String) -> PyResult<()> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let traceimport = py.import("edenscm.traceimport")?;
        traceimport.call(py, "enable", (pattern,), None)?;
        Ok(())
    }

    /// Run the Python interpreter.
    pub fn run_python(&mut self, args: &[String], io: &clidispatch::io::IO) -> u8 {
        let args = Self::prepare_args(args);
        if self.py_initialized_by_us {
            // Py_Main will call Py_Finalize. Therefore skip Py_Finalize here.
            self.py_initialized_by_us = false;
            py_main(&args)
        } else {
            // If Python is not initialized by us, it's expected that this
            // function does not call Py_Finalize.
            //
            // If we call Py_Main, users like the Python testutil, or the Python
            // chgserver will crash because Py_Main calls Py_Finalize.
            // Avoid that by just returning an error code.
            let _ = io.write_err("error: Py_Main cannot be used in this context\n");
            1
        }
    }
}

impl Drop for HgPython {
    fn drop(&mut self) {
        if self.py_initialized_by_us {
            info_span!("Finalize Python").in_scope(|| py_finalize())
        }
    }
}

fn read_to_py_object(py: Python, reader: &dyn clidispatch::io::Read) -> PyObject {
    let any = reader.as_any();
    if let Some(_) = any.downcast_ref::<std::io::Stdin>() {
        // The Python code accepts None, and will use its default input stream.
        py.None()
    } else if let Some(obj) = any.downcast_ref::<WrappedIO>() {
        obj.0.clone_ref(py)
    } else {
        unimplemented!(
            "converting non-stdio Read ({}) from Rust to Python is not implemented",
            reader.type_name()
        )
    }
}

fn write_to_py_object(py: Python, writer: &dyn clidispatch::io::Write) -> PyObject {
    let any = writer.as_any();
    if let Some(_) = any.downcast_ref::<std::io::Stdout>() {
        py.None()
    } else if let Some(_) = any.downcast_ref::<std::io::Stderr>() {
        py.None()
    } else if let Some(obj) = any.downcast_ref::<WrappedIO>() {
        obj.0.clone_ref(py)
    } else {
        unimplemented!(
            "converting non-stdio Write ({}) from Rust to Python is not implemented",
            writer.type_name()
        )
    }
}

fn init_bindings_commands(py: Python, package: &str) -> PyResult<PyModule> {
    // Called by chg or "-t.py" tests.
    fn run_py(
        py: Python,
        args: Vec<String>,
        fin: Option<PyObject>,
        fout: Option<PyObject>,
        ferr: Option<PyObject>,
    ) -> PyResult<i32> {
        if let (Some(fin), Some(fout), Some(ferr)) = (fin, fout, ferr) {
            let fin = wrap_pyio(fin);
            let fout = wrap_pyio(fout);
            let ferr = wrap_pyio(ferr);
            let old_io = IO::main();
            let io = IO::new(fin, fout, Some(ferr));
            io.set_main();
            let result = Ok(crate::run_command(args, &io));
            if let Ok(old_io) = old_io {
                old_io.set_main();
            }
            result
        } else {
            // Reuse the main IO.
            let io = IO::main().map_pyerr(py)?;
            Ok(crate::run_command(args, &io))
        }
    }

    fn table_py(py: Python) -> PyResult<PyDict> {
        let table = commands::table();
        let py_table: PyDict = PyDict::new(py);
        for def in table.values() {
            let doc = Str::from(Bytes::from(def.doc().to_string()));

            // Key entry by primary command name which Python knows to
            // look for. This avoids having to make the alias list
            // match exactly between Python and Rust.
            let primary_name = def.aliases().split('|').next().unwrap();

            if let Some(synopsis) = def
                .synopsis()
                .map(|s| Str::from(Bytes::from(s.to_string())))
            {
                py_table.set_item(py, primary_name, (doc, def.flags(), synopsis))?;
            } else {
                py_table.set_item(py, primary_name, (doc, def.flags()))?;
            }
        }
        Ok(py_table)
    }

    let name = [package, "commands"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add(
        py,
        "run",
        py_fn!(
            py,
            run_py(
                args: Vec<String>,
                fin: Option<PyObject> = None,
                fout: Option<PyObject> = None,
                ferr: Option<PyObject> = None,
            )
        ),
    )?;
    m.add(py, "table", py_fn!(py, table_py()))?;
    Ok(m)
}

/// Prepare builtin modules. Namely, `bindings`.
///
/// This makes sure the bindings use the same dependencies as the main
/// executable. For example, the global instance in `blackbox` is the
/// same, so if the Rust code logs to blackbox, the Python code can read
/// them out via bindings.
///
/// This is more difficult if the bindings project is compiled as a separate
/// Python extension, because `blackbox` will be compiled separately, and
/// the global instance might be different.
pub fn prepare_builtin_modules(py: Python<'_>, module: &PyModule) -> PyResult<()> {
    let span = debug_span!("Initialize bindings");
    let _guard = span.enter();

    // Prepare `bindings.command`. This cannot be done in the bindings
    // crate because it forms a circular dependency.
    module.add(
        py,
        "commands",
        init_bindings_commands(py, module.name(py)?)?,
    )?;
    bindings::populate_module(py, &module)?;
    Ok(())
}
