// Copyright Facebook, Inc. 2018
use crate::python::{
    py_finalize, py_init_threads, py_initialize, py_set_argv, py_set_program_name,
};
use cpython::{exc, NoArgs, ObjectProtocol, PyResult, Python};
use encoding::osstring_to_local_cstring;
use std;
use std::ffi::CString;

const HGPYENTRYPOINT_MOD: &str = "edenscm.mercurial.entrypoint";
pub struct HgPython {}

impl HgPython {
    pub fn new() -> HgPython {
        Self::setup_python();
        HgPython {}
    }

    fn setup_python() {
        let args = Self::args_to_local_cstrings();
        let executable_name = args[0].clone();
        py_set_program_name(executable_name);
        py_initialize();
        py_set_argv(args);
        py_init_threads();
    }

    fn args_to_local_cstrings() -> Vec<CString> {
        std::env::args_os()
            .map(|x| osstring_to_local_cstring(&x))
            .collect()
    }

    pub fn run_py(&self, py: Python<'_>) -> PyResult<()> {
        let entry_point_mod = py.import(HGPYENTRYPOINT_MOD)?;
        entry_point_mod.call(py, "run", NoArgs, None)?;
        Ok(())
    }

    pub fn run(&self) -> i32 {
        let gil = Python::acquire_gil();
        let py = gil.python();
        match self.run_py(py) {
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
                let is_system_exit = {
                    (&mut err)
                        .instance(py)
                        .extract::<exc::SystemExit>(py)
                        .is_ok()
                };
                let exit_code = {
                    let err = &mut err;
                    let inst = err.instance(py);
                    if is_system_exit {
                        match inst.getattr(py, "code") {
                            Ok(code) => code.extract::<i32>(py).unwrap(),
                            Err(_) => 255,
                        }
                    } else {
                        // Return value 1 is consistent with the Python interpreter.
                        // ex. `python -c "raise RuntimeError()"`
                        1
                    }
                };
                if !is_system_exit {
                    // Print a traceback
                    err.print(py);
                }
                exit_code
            }
            Ok(()) => 0,
        }
    }
}

impl Drop for HgPython {
    fn drop(&mut self) {
        py_finalize();
    }
}
