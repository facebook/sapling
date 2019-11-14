/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(feature = "python2")]
use libc::c_char;
use libc::c_int;
#[cfg(feature = "python3")]
use libc::wchar_t;

#[cfg(feature = "python2")]
use python27_sys::{
    PyEval_InitThreads, PySys_SetArgv, Py_Finalize, Py_Initialize, Py_IsInitialized, Py_Main,
    Py_SetProgramName,
};
#[cfg(feature = "python3")]
use python3_sys::{
    PyEval_InitThreads, PySys_SetArgv, PyUnicode_AsWideCharString, PyUnicode_FromString,
    Py_Finalize, Py_Initialize, Py_IsInitialized, Py_Main, Py_SetProgramName,
};
use std::ffi::CString;

#[cfg(feature = "python2")]
type PyChar = c_char;
#[cfg(feature = "python3")]
type PyChar = wchar_t;

#[cfg(feature = "python2")]
fn to_py_str(s: CString) -> *mut PyChar {
    s.into_raw()
}

#[cfg(feature = "python3")]
fn to_py_str(s: CString) -> *mut PyChar {
    unsafe {
        let pyobj = PyUnicode_FromString(s.as_ptr());
        PyUnicode_AsWideCharString(pyobj, std::ptr::null_mut())
    }
}

fn to_py_argv(args: Vec<CString>) -> Vec<*mut PyChar> {
    let mut argv: Vec<_> = args.into_iter().map(|x| to_py_str(x)).collect();
    argv.push(std::ptr::null_mut());
    argv
}

pub fn py_set_argv(args: Vec<CString>) {
    let mut argv = to_py_argv(args);
    unsafe {
        // This inserts argv[0] path to sys.path, useful for running local builds.
        PySys_SetArgv((argv.len() - 1) as c_int, argv.as_mut_ptr());
    }
    std::mem::forget(argv);
}

pub fn py_main(args: Vec<CString>) -> u8 {
    let mut argv = to_py_argv(args);
    let result = unsafe {
        let argc = (argv.len() - 1) as c_int;
        // Py_Main may not return.
        Py_Main(argc, argv.as_mut_ptr())
    };
    std::mem::forget(argv);
    result as u8
}

pub fn py_set_program_name(name: CString) {
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
        Py_Finalize();
    }
}

pub fn py_init_threads() {
    unsafe {
        PyEval_InitThreads();
    }
}
