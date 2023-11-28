/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ffi::CStr;
use std::ffi::CString;

use ffi::PyEval_InitThreads;
use ffi::PyGILState_Ensure;
use ffi::PyUnicode_AsWideCharString;
use ffi::PyUnicode_FromString;
use ffi::Py_DECREF;
use ffi::Py_Finalize;
use ffi::Py_IsInitialized;
use ffi::Py_Main;
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
            let s = CString::new(s.as_ref()).unwrap();
            ffi::Py_DecodeLocale(s.as_ptr(), std::ptr::null_mut())
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

pub fn py_finalize() {
    unsafe {
        // Stop other threads from running Python logic.
        //
        // During Py_Finalize, other Python threads might pthread_exit and funny things might
        // happen with rust-cpython's unwind protection.  Example SIGABRT stacks:
        //
        // Thread 1 (exiting):
        // #0  ... in raise () from /lib64/libc.so.6
        // #1  ... in abort () from /lib64/libc.so.6
        // #2  ... in __libc_message () from /lib64/libc.so.6
        // #3  ... in __libc_fatal () from /lib64/libc.so.6
        // #4  ... in unwind_cleanup () from /lib64/libpthread.so.0
        // #5  ... in panic_unwind::real_imp::cleanup ()
        //     at library/panic_unwind/src/gcc.rs:78
        // #6  panic_unwind::__rust_panic_cleanup ()
        //     at library/panic_unwind/src/lib.rs:100
        // #7  ... in std::panicking::try::cleanup ()
        //     at library/std/src/panicking.rs:360
        // #8  ... in std::panicking::try::do_catch (data=<optimized out>, payload=... "\000")
        //     at library/std/src/panicking.rs:404
        // #9  std::panicking::try (f=...) at library/std/src/panicking.rs:343
        // #10 ... in std::panic::catch_unwind (f=...)
        //     at library/std/src/panic.rs:396
        // #11 cpython::function::handle_callback (_location=..., _c=..., f=...)
        //     at cpython-0.5.1/src/function.rs:216
        // #12 ... in pythreading::RGeneratorIter::create_instance::TYPE_OBJECT::wrap_unary (slf=...)
        //     at cpython-0.5.1/src/py_class/slots.rs:318
        // #13 ... in builtin_next () from /lib64/libpython3.6m.so.1.0
        // #14 ... in call_function () from /lib64/libpython3.6m.so.1.0
        // #15 ... in _PyEval_EvalFrameDefault () from /lib64/libpython3.6m.so.1.0
        // ....
        // #32 ... in PyObject_Call () from /lib64/libpython3.6m.so.1.0
        // #33 ... in t_bootstrap () from /lib64/libpython3.6m.so.1.0
        // #34 ... in pythread_wrapper () from /lib64/libpython3.6m.so.1.0
        // #35 ... in start_thread () from /lib64/libpthread.so.0
        // #36 ... in clone () from /lib64/libc.so.6
        //
        // Thread 2:
        // #0  ... in _int_free () from /lib64/libc.so.6
        // #1  ... in code_dealloc () from /lib64/libpython3.6m.so.1.0
        // #2  ... in func_dealloc () from /lib64/libpython3.6m.so.1.0
        // #3  ... in PyObject_ClearWeakRefs () from /lib64/libpython3.6m.so.1.0
        // #4  ... in subtype_dealloc () from /lib64/libpython3.6m.so.1.0
        // #5  ... in insertdict () from /lib64/libpython3.6m.so.1.0
        // #6  ... in _PyModule_ClearDict () from /lib64/libpython3.6m.so.1.0
        // #7  ... in PyImport_Cleanup () from /lib64/libpython3.6m.so.1.0
        // #8  ... in Py_FinalizeEx () from /lib64/libpython3.6m.so.1.0
        // #9  ... in hgcommands::python::py_finalize ()
        // ....
        // #15 ... in hgmain::main () eden/scm/exec/hgmain/src/main.rs:81
        //
        // (The SIGABRT was triggered by running test-fb-ext-fastlog.t)
        PyGILState_Ensure();
        Py_Finalize();
    }
}

pub fn py_init_threads() {
    unsafe {
        PyEval_InitThreads();
    }
}
