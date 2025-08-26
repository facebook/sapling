/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ffi::CStr;
use std::ffi::CString;
use std::sync::LazyLock;

use ffi::Py_DECREF;
use ffi::Py_IsInitialized;
use ffi::Py_Main;
use ffi::PyEval_InitThreads;
use ffi::PyUnicode_AsWideCharString;
use ffi::PyUnicode_FromString;
use libc::c_int;
use libc::wchar_t;
use python3_sys as ffi;

type PyChar = wchar_t;

fn to_py_str(s: &str) -> *mut PyChar {
    let c_str = CString::new(s).unwrap();

    unsafe {
        let unicode_obj = PyUnicode_FromString(c_str.as_ptr());
        assert!(!unicode_obj.is_null());

        let py_str = PyUnicode_AsWideCharString(unicode_obj, std::ptr::null_mut());

        Py_DECREF(unicode_obj);

        py_str
    }
}

fn to_py_argv(args: &[String]) -> Vec<*mut PyChar> {
    let mut argv: Vec<_> = args.iter().map(|x| to_py_str(x)).collect();
    argv.push(std::ptr::null_mut());
    argv
}

pub fn py_main(args: &[String]) -> u8 {
    let mut argv = to_py_argv(args);
    unsafe {
        let argc = (argv.len() - 1) as c_int;
        // Py_Main may not return.
        Py_Main(argc, argv.as_mut_ptr()) as u8
    }
}

macro_rules! check_status {
    ($status: expr, $config: expr) => {
        let status = $status;
        if ffi::PyStatus_Exception(status) != 0 {
            if let Some(mut config) = $config {
                ffi::PyConfig_Clear(&mut config);
            }
            ffi::Py_ExitStatusException(status);
            unreachable!();
        }
    };
}

fn frozen_module(cname: &'static [u8]) -> ffi::_frozen {
    let name = std::str::from_utf8(cname).unwrap().trim_end_matches('\0');
    let module = match python_modules::find_module(name) {
        None => panic!("module {} should be included", name),
        Some(v) => v,
    };
    let code = module.byte_code();
    let mut frozen = unsafe { std::mem::zeroed::<ffi::_frozen>() };
    frozen.name = cname.as_ptr() as _;
    frozen.code = code.as_ptr() as _;
    frozen.size = code.len() as _;
    // `is_package` requires Python 3.12
    #[cfg(python_since_3_12)]
    {
        frozen.is_package = module.is_package() as _;
    }
    // Python < 3.12 uses a negative "size" to indicate a package.
    #[cfg(not(python_since_3_12))]
    {
        if module.is_package() {
            frozen.size = -frozen.size;
        }
    }
    frozen
}

struct ForceSend<T>(T);
unsafe impl<T> Send for ForceSend<T> {}
unsafe impl<T> Sync for ForceSend<T> {}

// Pure Python modules imported from disk during `Py_Initialize`.
//
// Modern Pythons uses "frozen" modules to reduce startup overhead by not importing from disk.
// However, they have some left-overs for compatibility reasons. Namely, the `encodings` modules
// aren't frozen (https://github.com/python/cpython/issues/89816).
//
// We don't care about the compatibility. Provide those modules to avoid disk access.
//
// To get a list of the modules loaded from disk, run:
//
// ```bash,ignore
// python -S -v -c pass |& grep 'code object from'
// ```
static EXTRA_FROZEN_MODULES: LazyLock<ForceSend<[ffi::_frozen; 5]>> = LazyLock::new(|| {
    ForceSend([
        frozen_module(b"encodings\0"),
        frozen_module(b"encodings.aliases\0"),
        frozen_module(b"encodings.utf_8\0"),
        frozen_module(b"linecache\0"), // used by Python >= 3.13
        unsafe { std::mem::zeroed::<ffi::_frozen>() },
    ])
});

/// Initialize Python interpreter given args and optional Sapling Python home.
/// `args[0]` is the executable name to be used. If specified, `sapling_home`
/// points to the directory containing the "sapling" Python package. This allows
/// Sapling Python modules to be loaded from disk during development.
#[allow(unexpected_cfgs)]
pub fn py_initialize(args: &[String], sapling_home: Option<&String>) {
    unsafe {
        let mut pre_config = ffi::PyPreConfig::default();
        ffi::PyPreConfig_InitPythonConfig(&mut pre_config);

        pre_config.parse_argv = 0;
        pre_config.utf8_mode = 1;

        check_status!(ffi::Py_PreInitialize(&pre_config), None);

        let mut config = ffi::PyConfig::default();

        // Ideally we could use PyConfig_InitIsolatedConfig, but we rely on some
        // of the vanilla initialization logic to find the std lib, at least.
        ffi::PyConfig_InitPythonConfig(&mut config);

        config.install_signal_handlers = 0;
        config.site_import = 0;
        config.parse_argv = 0;

        // This allows IPython to be installed in user site dir.
        config.user_site_directory = 1;

        // This assumes Python has been pre-initialized, and filesystem encoding
        // is utf-8 (both done above).
        unsafe fn to_wide(s: impl AsRef<str>) -> *const PyChar {
            unsafe {
                let s = CString::new(s.as_ref()).unwrap();
                ffi::Py_DecodeLocale(s.as_ptr(), std::ptr::null_mut())
            }
        }

        check_status!(
            ffi::PyConfig_SetString(&mut config, &mut config.executable, to_wide(&args[0])),
            Some(config)
        );

        for arg in args.iter() {
            check_status!(
                ffi::PyWideStringList_Append(&mut config.argv, to_wide(arg)),
                Some(config)
            );
        }

        // For static libpython, set prefix be the current exe directory. This allows us to package
        // the main binary with Python stdlib (ex. python312.zip, and native modules) to not
        // dependent on a particular version of the system python.
        #[cfg(static_libpython)]
        {
            let exe_path = std::env::current_exe().unwrap();
            let exe_dir = exe_path.parent().unwrap();
            let exe_dir = exe_dir.to_str().expect("utf-8 exe dir");
            // "prefix" affects many other paths.
            // See https://docs.python.org/3/c-api/init_config.html#init-path-config
            check_status!(
                ffi::PyConfig_SetString(&mut config, &mut config.prefix, to_wide(exe_dir)),
                Some(config)
            );
        }

        check_status!(ffi::PyConfig_Read(&mut config), Some(config));

        // "3.10.9 (v3.10.9:1dd9be6584, Dec  6 2022, 14:37:36) [Clang 13.0.0 (clang-1300.0.29.30)]"
        let version = CStr::from_ptr(ffi::Py_GetVersion());
        let minor_version: Option<u8> = version
            .to_string_lossy()
            .strip_prefix("3.")
            .and_then(|v| v.split_once(|c: char| !c.is_ascii_digit()))
            .and_then(|(v, _)| v.parse().ok());

        // In Python 3.10 we need to set `config.module_search_paths_set = 1` or
        // else Py_Main (for "debugpython") always overwrites sys.path.
        //
        // In Python 3.11, Py_Main doesn't clobber sys.path, so our
        // sys.path.append(SAPLING_PYTHON_HOME) takes effect in
        // HgPython::update_meta_path.
        if minor_version == Some(10) {
            if let Some(home) = sapling_home {
                // This tells Py_Main to not overwrite sys.path and to copy our below value.
                config.module_search_paths_set = 1;
                check_status!(
                    ffi::PyWideStringList_Append(&mut config.module_search_paths, to_wide(home)),
                    Some(config)
                );
            }
        }

        // NOTE: Fails with fbcode buck build that links to libpython3.10: P1830059652.
        // Try again after upgrading to Python 3.12.
        if cfg!(static_libpython) {
            ffi::PyImport_FrozenModules = EXTRA_FROZEN_MODULES.0.as_ptr() as _;
        }

        check_status!(ffi::Py_InitializeFromConfig(&config), Some(config));

        ffi::PyConfig_Clear(&mut config);
    }
}

pub fn py_is_initialized() -> bool {
    unsafe { Py_IsInitialized() != 0 }
}

pub fn py_init_threads() {
    unsafe {
        PyEval_InitThreads();
    }
}
