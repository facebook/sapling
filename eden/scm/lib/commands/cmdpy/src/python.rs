/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ffi::CStr;
use std::ffi::CString;

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
    ($status: expr_2021, $config: expr_2021) => {
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

/// Initialize Python interpreter given args and optional Sapling Python home.
/// `args[0]` is the executable name to be used. If specified, `sapling_home`
/// points to the directory containing the "sapling" Python package. This allows
/// Sapling Python modules to be loaded from disk during development.
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
