// Copyright Facebook, Inc. 2018
use crate::commands;
use crate::python::{
    py_finalize, py_init_threads, py_initialize, py_is_initialized, py_set_argv,
    py_set_program_name,
};
use cpython::{
    exc, ObjectProtocol, PyClone, PyDict, PyObject, PyResult, Python, PythonObject, ToPyObject,
};
use cpython_ext::{Bytes, WrappedIO};
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

    fn run_py(
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

    pub fn run(&self, args: Vec<String>, io: &mut clidispatch::io::IO) -> i32 {
        let gil = Python::acquire_gil();
        let py = gil.python();
        match self.run_py(py, args, io) {
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
