// Copyright Facebook, Inc. 2018
use crate::buildenv::BuildEnv;
use crate::python::{
    py_finalize, py_init_threads, py_initialize, py_set_argv, py_set_no_site_flag,
    py_set_program_name, py_set_python_home,
};
use cpython::{exc, ObjectProtocol, PyBytes, PyObject, PyResult, Python};
use encoding::{osstring_to_local_cstring, path_to_local_bytes, path_to_local_cstring};
use std;
use std::ffi::{CString, OsStr, OsString};
use std::path::{Path, PathBuf};

/// A default name of the python script that this Rust binary will try to
/// load when it decides to pass control to Python
const HGPYENTRYPOINT_MOD: &str = "entrypoint";
/// A path to the entrypoint module in the installation of Python/Mercurial
#[cfg(target_family = "unix")]
const ENTRYPOINT_IN_INSTALLATION: &str = "edenscm/mercurial/entrypoint.py";
#[cfg(target_family = "windows")]
const ENTRYPOINT_IN_INSTALLATION: &str = "edenscm\\mercurial\\entrypoint.py";

/// A default path to the zipped Python/Mercurial library for the
/// embedded case
#[cfg(target_family = "unix")]
const EMBEDDED_LIBRARY: &str = "lib/library.zip";
#[cfg(target_family = "windows")]
const EMBEDDED_LIBRARY: &str = "lib\\library.zip";

/// Embedded/wrapped Python struct
/// # Rationale
/// This struct is an abstraction that let's Rust Mercurial
/// invoke Python logic (either completely pass control or, potentially,
/// just call some functions).
/// Using Python form Rust Mercurial covers two approaches, which I
/// call using and embedding.
/// # Using
/// Using happens when we are using Python installed somewhere on
/// the file system. We just link with the Python shared library, perform
/// some initialization and expect Python to figure out the rest correctly.
/// This is an expected approach for Linux and OSX.
/// # Embedding
/// Embedding allows us to load Python's standard library (pure python bits
/// of it) from a zip file. This leads to a higher speed on Windows. Unfortunately,
/// it is impossible to load .pyd's from a zip file, so they have
/// to be handled separately.
/// # Behavior
/// This struct is pretty flexible in terms of what it can work with. All
/// the decision making is around finding the location of `edenscm/mercurial/entrypoint.py`
/// file, which is a place to pass control to. Here are the list of candidates
/// for this file:
/// - `BIANRY_LOCATION/lib/library.zip/edenscm/mercurial/entrypoint.py`
/// - `BINARY_LOCATION/edenscm/mercurial/entrypotint.py`
/// - `edenscm/mercurial/entrypoint.py` under each of directories, supplied at
///   compile time via the `HGPYENTRYPOINTSEARCHPATH` environment variable
/// - a few hardcoded locations
/// The reason so many locations are investigated is because this binary is
/// intended to work in many scenarios:
/// - from repo after `make local` on all platforms, when non-embedded workings
///   are neede and `edenscm/mercurial/entrypoint.py` lives alongside the main binary
/// - installed on Window, when lib/library.zip lives alongsige with the main binary
/// - installed on *NIX, when `edenscm/mercurail/entrypoint.py` lives in `site-packages`
///   of the main Python installation
/// - installed on *NIX, when `edenscm/mercurial/entrypoint.py` lives in some non-standard
///   location
pub struct HgPython {
    embedded: bool,
    entry_point: PathBuf,
}

impl HgPython {
    pub fn new() -> HgPython {
        let exe_path = std::env::current_exe()
            .expect("failed to call current_exe")
            .canonicalize()
            .expect("failed to canonicalize current_exe");
        let installation_root = exe_path.parent().unwrap();
        let env = BuildEnv::new();
        let entry_point = Self::find_hg_py_entry_point(&installation_root);
        let embedded = Self::is_embedded(&entry_point);
        Self::setup_python(&installation_root, &entry_point, embedded, &env);

        HgPython {
            embedded,
            entry_point,
        }
    }

    /// Setup everything related to the python interpreter
    /// used by Mercurial
    fn setup_python(installation_root: &Path, entry_point: &Path, embedded: bool, env: &BuildEnv) {
        if embedded {
            // In an embedded case, we don't need the site.py logic, as
            // we don't need any filesystem discovery: we know the location
            // of all the packages in advance.
            py_set_no_site_flag();
        } else if cfg!(target_os = "windows") {
            let hgpython: OsString = env
                .var_os("HGPYTHONHOME")
                .unwrap_or(installation_root.join("hg-python").into());
            py_set_python_home(&hgpython);
        }

        let mut args = Self::args_to_local_cstrings();
        let executable_name = args[0].clone();
        py_set_program_name(executable_name);
        py_initialize();
        args[0] = path_to_local_cstring(&entry_point);
        py_set_argv(args);
        py_init_threads();
    }

    fn entry_point_in_installation<P: AsRef<Path>>(installation: P) -> PathBuf {
        installation.as_ref().join(ENTRYPOINT_IN_INSTALLATION)
    }

    fn zip_in_installation<P: AsRef<Path>>(installation: P) -> PathBuf {
        installation.as_ref().join(EMBEDDED_LIBRARY)
    }

    fn is_zip_file<P: AsRef<Path>>(path: P) -> bool {
        path.as_ref().extension() == Some(OsStr::new("zip"))
    }

    /// Return the zipfile base of a path if run from a zipfile
    /// Example: get_zip_base('/a/b.zip/mercurial/entrypoint.py')
    /// is '/a/b.zip'
    fn get_zip_base(path: &Path) -> Option<&Path> {
        let mut path = path.as_ref();
        // We can be at any location in the .zip file:
        // - library.zip/mercurial/entrypoint.py is acceptable
        // - library.zip/some-dir/mercurial/entrypoinr.py is also acceptable
        loop {
            if Self::is_zip_file(&path) {
                return Some(path);
            }
            path = match path.parent() {
                Some(parent) => parent,
                None => {
                    return None;
                }
            }
        }
    }

    /// Detect if Mercurial is run in an embedded mode
    /// We are in an embedded mode if all Python files are stored
    /// in a zipfile.
    fn is_embedded<P: AsRef<Path>>(entry_point: P) -> bool {
        Self::get_zip_base(entry_point.as_ref()) != None
    }

    /// Check if the entrypoint candidate looks like an actual entrypoint
    ///
    /// Either the candidate itself should exist, or its zip base should
    fn is_suitable_candidate<P: AsRef<Path>>(candidate: P) -> bool {
        let candidate = candidate.as_ref();
        candidate.exists()
            || match Self::get_zip_base(candidate) {
                None => false,
                Some(zip) => zip.exists(),
            }
    }

    /// Detect the entry point Python script for current Mercurial run
    fn find_hg_py_entry_point(installation_root: &Path) -> PathBuf {
        let mut candidates: Vec<PathBuf> = vec![];

        // Pri 1: the zip file under the dir where the binary lives (embedded case)
        candidates.push(Self::entry_point_in_installation(
            &Self::zip_in_installation(&installation_root),
        ));

        // Pri 2: the dir where the binary lives
        candidates.push(Self::entry_point_in_installation(&installation_root));

        // TODO: Pri 3: read the config file, which may specify the entrypoint location

        // Pri 4: a list of compile-time provided paths to check
        // Note that HGPYENTRYPOINTSEARCHPATH is in a PATH format and each item is
        // expected to end in mercurial/
        if let Some(compile_time_locations) = option_env!("HGPYENTRYPOINTSEARCHPATH") {
            for path in std::env::split_paths(compile_time_locations) {
                candidates.push(Self::entry_point_in_installation(&path));
            }
        }

        // Pri 5: a list of source-level hardcoded paths to check
        vec![
            &PathBuf::from("/usr/lib64/python2.7/site-packages/"),
            &PathBuf::from("/usr/lib/python2.7/site-packages/"),
        ]
        .iter()
        .for_each(|pb| candidates.push(Self::entry_point_in_installation(pb)));

        for candidate in candidates.iter() {
            if Self::is_suitable_candidate(&candidate) {
                return candidate.clone();
            }
        }
        panic!(
            "could not find {} in {:?}",
            ENTRYPOINT_IN_INSTALLATION, candidates
        );
    }

    fn args_to_local_cstrings() -> Vec<CString> {
        std::env::args_os()
            .map(|x| osstring_to_local_cstring(&x))
            .collect()
    }

    /// Given a `sys.path` Python list, add a `path` component there
    fn add_to_sys_path<P: AsRef<Path>>(
        &self,
        py: Python<'_>,
        sys_path: &PyObject,
        path: P,
        index: u32,
    ) -> PyResult<()> {
        let to_insert = PyBytes::new(py, &path_to_local_bytes(&path.as_ref()).unwrap());
        sys_path.call_method(py, "insert", (index, to_insert), None)?;
        Ok(())
    }

    /// Prepare Python `sys.path` to run Mercurial
    fn adjust_path(&self, py: Python<'_>) -> PyResult<()> {
        let sys_mod = py.import("sys").unwrap();
        let sys_path = sys_mod.get(py, "path").unwrap();
        self.add_to_sys_path(py, &sys_path, &self.entry_point.parent().unwrap(), 0)?;
        if self.embedded {
            match Self::get_zip_base(&self.entry_point.as_path()) {
                None => panic!(
                    "Should be impossible: embedded Python, yet no zip in the entrypoint path"
                ),
                Some(zip) => {
                    // sys.path should contain both the .zip file and it's parent
                    // directory, because that's where .pyd's are stored.
                    self.add_to_sys_path(py, &sys_path, &zip, 1)?;
                    self.add_to_sys_path(py, &sys_path, &zip.parent().unwrap(), 2)?;
                }
            }
        }
        Ok(())
    }

    pub fn run_py(&self, py: Python<'_>) -> PyResult<()> {
        self.adjust_path(py)?;
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
                    (&mut err)
                        .instance(py)
                        .extract::<exc::SystemExit>(py)
                        .is_ok()
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
