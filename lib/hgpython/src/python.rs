// Copyright Facebook, Inc. 2018
use encoding::path_to_local_cstring;
use libc::c_int;
use python27_sys::{PyEval_InitThreads, PySys_SetArgvEx, Py_Finalize, Py_Initialize, Py_NoSiteFlag,
                   Py_SetProgramName, Py_SetPythonHome};
use std;
use std::ffi::CString;
use std::path::Path;

pub fn py_set_python_home<P: AsRef<Path>>(path: &P) {
    let path = path_to_local_cstring(path.as_ref());
    unsafe { Py_SetPythonHome(path.into_raw()) };
}

pub fn py_set_argv(args: Vec<CString>) {
    let mut argv: Vec<_> = args.into_iter().map(|x| x.into_raw()).collect();
    argv.push(std::ptr::null_mut());
    unsafe {
        PySys_SetArgvEx((argv.len() - 1) as c_int, argv.as_mut_ptr(), 0);
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

pub fn py_set_no_site_flag() {
    unsafe {
        Py_NoSiteFlag = 1 as c_int;
    }
}
