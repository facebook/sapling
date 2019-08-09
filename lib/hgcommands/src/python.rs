// Copyright Facebook, Inc. 2018

use libc::c_int;
use python27_sys::{
    PyEval_InitThreads, PySys_SetArgv, Py_Finalize, Py_Initialize, Py_SetProgramName,
};
use std;
use std::ffi::CString;

pub fn py_set_argv(args: Vec<CString>) {
    let mut argv: Vec<_> = args.into_iter().map(|x| x.into_raw()).collect();
    argv.push(std::ptr::null_mut());
    unsafe {
        // This inserts argv[0] path to sys.path, useful for running local builds.
        PySys_SetArgv((argv.len() - 1) as c_int, argv.as_mut_ptr());
    }
    std::mem::forget(argv);
}

pub fn py_set_program_name(name: CString) {
    unsafe {
        Py_SetProgramName(name.into_raw());
    }
}

pub fn py_initialize() {
    unsafe {
        Py_Initialize();
    }
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
