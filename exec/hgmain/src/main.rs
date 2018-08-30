extern crate cpython;
extern crate encoding;
extern crate libc;
extern crate python27_sys;

mod python;
use cpython::{ObjectProtocol, PyBytes, PyResult, Python, exc};
use encoding::{osstring_to_local_cstring, path_to_local_bytes, path_to_local_cstring};
use python::{py_finalize, py_init_threads, py_initialize, py_set_argv, py_set_program_name,
             py_set_python_home};
use std::env;
use std::ffi::CString;
use std::path::{Path, PathBuf};

/// A default name of the python script that this Rust binary will try to
/// load when it decides to pass control to Python
const HGPYENTRYPOINT_PY: &str = "entrypoint.py";
const HGPYENTRYPOINT_MOD: &str = "entrypoint";

struct HgPython {
    entry_point: PathBuf,
}

impl HgPython {
    pub fn new() -> HgPython {
        let exe_path = env::current_exe().expect("failed to call current_exe");
        let installation_root = exe_path.parent().unwrap();
        let entry_point = Self::find_hg_py_entry_point(&installation_root);
        Self::setup(&installation_root, &entry_point);

        HgPython { entry_point }
    }

    fn setup(installation_root: &Path, entry_point: &Path) {
        if cfg!(target_os = "windows") {
            py_set_python_home(&installation_root.join("hg-python"));
        }
        let mut args = Self::args_to_local_cstrings();
        let executable_name = args[0].clone();
        py_set_program_name(executable_name);
        py_initialize();
        args[0] = path_to_local_cstring(&entry_point);
        py_set_argv(args);
        py_init_threads();
    }

    fn find_hg_py_entry_point(installation_root: &Path) -> PathBuf {
        let mut candidates: Vec<PathBuf> = vec![];

        // Pri 0: entry point from the environment is a file, not a dir
        if let Ok(env_entry_point) = env::var("HGPYENTRYPOINT") {
            candidates.push(PathBuf::from(env_entry_point));
        }

        // Pri 1: the dir where the binary lives
        candidates.push(installation_root.join("mercurial").join(HGPYENTRYPOINT_PY));

        // TODO: Pri 2: read the config file, which may specify the entrypoint location

        // Pri 3: a list of compile-time provided paths to check
        // Note that HGPYENTRYPOINTSEARCHPATH is in a PATH format and each item is
        // expected to end in mercurial/
        if let Some(compile_time_locations) = option_env!("HGPYENTRYPOINTSEARCHPATH") {
            for path in env::split_paths(compile_time_locations) {
                candidates.push(path.join(HGPYENTRYPOINT_PY));
            }
        }

        // Pri 4: a list of source-level hardcoded paths to check
        candidates.push(
            PathBuf::from("/usr/lib64/python2.7/site-packages/mercurial/").join(HGPYENTRYPOINT_PY),
        );
        candidates.push(
            PathBuf::from("/usr/lib/python2.7/site-packages/mercurial/").join(HGPYENTRYPOINT_PY),
        );

        for candidate in candidates.iter() {
            if candidate.exists() {
                return candidate.clone();
            }
        }
        panic!("could not find {} in {:?}", HGPYENTRYPOINT_PY, candidates);
    }

    fn args_to_local_cstrings() -> Vec<CString> {
        env::args_os()
            .map(|x| osstring_to_local_cstring(&x))
            .collect()
    }

    pub fn run_py(&self, py: Python) -> PyResult<()> {
        let sys_mod = py.import("sys").unwrap();
        let sys_path = sys_mod.get(py, "path").unwrap();
        let to_insert = PyBytes::new(
            py,
            &path_to_local_bytes(&self.entry_point.parent().unwrap()).unwrap(),
        );
        sys_path
            .call_method(py, "insert", (0, to_insert), None)
            .expect("failed to update sys.path to location of Mercurial modules");
        let entry_point_mod = py.import(HGPYENTRYPOINT_MOD)?;
        entry_point_mod.call(py, "run", (py.True(),), None)?;
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
                    (&mut err).instance(py).extract::<exc::SystemExit>(py).is_ok()
                };
                let exit_code = {
                    let mut err = &mut err;
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

fn main() {
    let code = {
        let hgpython = HgPython::new();
        hgpython.run()
    };
    std::process::exit(code);
}
