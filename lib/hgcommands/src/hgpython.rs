// Copyright Facebook, Inc. 2018
use crate::commands;
use crate::python::{
    py_finalize, py_init_threads, py_initialize, py_is_initialized, py_main, py_set_argv,
    py_set_program_name,
};
use clidispatch::io::IO;
use cpython::*;
use cpython_ext::{wrap_pyio, Bytes, WrappedIO};
use encoding::osstring_to_local_cstring;
use std::env;
use std::ffi::CString;

const HGPYENTRYPOINT_MOD: &str = "edenscm.mercurial.entrypoint";
pub struct HgPython {
    py_initialized_by_us: bool,
}

impl HgPython {
    pub fn new(args: Vec<String>) -> HgPython {
        let py_initialized_by_us = !py_is_initialized();
        if py_initialized_by_us {
            Self::setup_python(args);
        }
        HgPython {
            py_initialized_by_us,
        }
    }

    fn setup_python(args: Vec<String>) {
        let args = Self::args_to_local_cstrings(args);
        let executable_name = args[0].clone();
        py_set_program_name(executable_name);
        py_initialize();
        py_set_argv(args);
        py_init_threads();

        let gil = Python::acquire_gil();
        let py = gil.python();

        // If this fails, it's a fatal error.
        prepare_builtin_modules(py).unwrap();
    }

    fn args_to_local_cstrings(args: Vec<String>) -> Vec<CString> {
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
        Some(env::current_exe().unwrap().into_os_string())
            .into_iter()
            .chain(args.into_iter().skip(1).map(Into::into))
            .map(|x| osstring_to_local_cstring(&x))
            .collect()
    }

    fn run_hg_py(
        &self,
        py: Python<'_>,
        args: Vec<String>,
        io: &mut clidispatch::io::IO,
    ) -> PyResult<()> {
        self.update_python_command_table(py)?;
        let entry_point_mod = py.import(HGPYENTRYPOINT_MOD)?;
        let call_args = {
            let fin = read_to_py_object(py, &io.input);
            let fout = write_to_py_object(py, &io.output);
            let ferr = match io.error {
                None => fout.clone_ref(py),
                Some(ref error) => write_to_py_object(py, error),
            };
            let args: Vec<Bytes> = args.into_iter().map(Bytes::from).collect();
            (args, fin, fout, ferr).to_py_object(py)
        };
        entry_point_mod.call(py, "run", call_args, None)?;
        Ok(())
    }

    /// Update the Python command table so it knows commands implemented in Rust.
    fn update_python_command_table(&self, py: Python<'_>) -> PyResult<()> {
        let table = commands::table();
        let table_mod = py.import("edenscm.mercurial.commands")?;
        let py_table: PyDict = table_mod.get(py, "table")?.extract::<PyDict>(py)?;

        for def in table.values() {
            let doc = Bytes::from(def.doc().to_string());
            py_table.set_item(py, def.name(), (doc, def.flags()))?;
        }

        Ok(())
    }

    /// Run an hg command defined in Python.
    pub fn run_hg(&self, args: Vec<String>, io: &mut clidispatch::io::IO) -> i32 {
        let gil = Python::acquire_gil();
        let py = gil.python();
        match self.run_hg_py(py, args, io) {
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
                    // Print a traceback
                    err.print(py);
                    1
                }
            }
            Ok(()) => 0,
        }
    }

    /// Run the Python interpreter.
    pub fn run_python(&mut self, args: Vec<String>, io: &mut clidispatch::io::IO) -> u8 {
        let args = Self::args_to_local_cstrings(args);
        if self.py_initialized_by_us {
            // Py_Main will call Py_Finalize. Therefore skip Py_Finalize here.
            self.py_initialized_by_us = false;
            py_main(args)
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
            py_finalize();
        }
    }
}

fn read_to_py_object(py: Python, reader: &Box<dyn clidispatch::io::Read>) -> PyObject {
    let any = Box::as_ref(reader).as_any();
    if let Some(_) = any.downcast_ref::<std::io::Stdin>() {
        // The Python code accepts None, and will use its default input stream.
        py.None()
    } else if let Some(obj) = any.downcast_ref::<WrappedIO>() {
        obj.0.clone_ref(py)
    } else {
        unimplemented!("converting non-stdio Read from Rust to Python is not implemented")
    }
}

fn write_to_py_object(py: Python, writer: &Box<dyn clidispatch::io::Write>) -> PyObject {
    let any = Box::as_ref(writer).as_any();
    if let Some(_) = any.downcast_ref::<std::io::Stdout>() {
        py.None()
    } else if let Some(_) = any.downcast_ref::<std::io::Stderr>() {
        py.None()
    } else if let Some(obj) = any.downcast_ref::<WrappedIO>() {
        obj.0.clone_ref(py)
    } else {
        unimplemented!("converting non-stdio Write from Rust to Python is not implemented")
    }
}

fn init_bindings_commands(py: Python, package: &str) -> PyResult<PyModule> {
    fn run_py(
        _py: Python,
        args: Vec<String>,
        fin: PyObject,
        fout: PyObject,
        ferr: Option<PyObject>,
    ) -> PyResult<i32> {
        let fin = wrap_pyio(fin);
        let fout = wrap_pyio(fout);
        let ferr = ferr.map(wrap_pyio);

        let mut io = IO::new(fin, fout, ferr);
        Ok(crate::run_command(args, &mut io))
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
                fin: PyObject,
                fout: PyObject,
                ferr: Option<PyObject> = None
            )
        ),
    )?;
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
fn prepare_builtin_modules(py: Python<'_>) -> PyResult<()> {
    let name = "bindings";
    let bindings_module = PyModule::new(py, &name)?;

    // Prepare `bindings.command`. This cannot be done in the bindings
    // crate because it forms a circular dependency.
    bindings_module.add(py, "commands", init_bindings_commands(py, name)?)?;
    bindings::populate_module(py, &bindings_module)?;

    // Putting the module in sys.modules makes it importable.
    let sys = py.import("sys")?;
    let sys_modules = PyDict::extract(py, &sys.get(py, "modules")?)?;
    sys_modules.set_item(py, name, bindings_module)?;
    Ok(())
}
