/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ffi::CString;

use ffi::PyEval_InitThreads;
use ffi::PyGILState_Ensure;
use ffi::PySys_SetArgv;
#[cfg(feature = "python3")]
use ffi::PyUnicode_AsWideCharString;
#[cfg(feature = "python3")]
use ffi::PyUnicode_FromString;
use ffi::Py_DECREF;
use ffi::Py_Finalize;
use ffi::Py_Initialize;
use ffi::Py_IsInitialized;
use ffi::Py_Main;
use ffi::Py_SetProgramName;
#[cfg(feature = "python2")]
use libc::c_char;
use libc::c_int;
#[cfg(feature = "python3")]
use libc::wchar_t;
#[cfg(feature = "python2")]
use python27_sys as ffi;
#[cfg(feature = "python3")]
use python3_sys as ffi;

#[cfg(feature = "python2")]
type PyChar = c_char;
#[cfg(feature = "python3")]
type PyChar = wchar_t;

#[cfg(feature = "python2")]
fn to_py_str(s: &str) -> *mut PyChar {
    unimplemented!()
}

#[cfg(feature = "python3")]
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

pub fn py_set_argv(args: &[String]) {
    let mut argv = to_py_argv(args);
    unsafe {
        // This inserts argv[0] path to sys.path, useful for running local builds.
        PySys_SetArgv((argv.len() - 1) as c_int, argv.as_mut_ptr());
    }
}

pub fn py_main(args: &[String]) -> u8 {
    let mut argv = to_py_argv(args);
    unsafe {
        let argc = (argv.len() - 1) as c_int;
        // Py_Main may not return.
        Py_Main(argc, argv.as_mut_ptr()) as u8
    }
}

pub fn py_set_program_name(name: &str) {
    unsafe {
        Py_SetProgramName(to_py_str(name));
    }
}

pub fn py_initialize() {
    unsafe {
        Py_Initialize();
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
        // (The SIGABRT was triggered by running test-fb-hgext-fastlog.t)
        PyGILState_Ensure();
        Py_Finalize();
    }
}

pub fn py_init_threads() {
    unsafe {
        PyEval_InitThreads();
    }
}
