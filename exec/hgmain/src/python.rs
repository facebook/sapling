use encoding::path_to_local_cstring;
use libc::c_int;
use python27_sys::{Py_Main, Py_SetPythonHome};
use std;
use std::ffi::CString;
use std::path::Path;

pub fn py_main(args: Vec<CString>) -> i32 {
    let mut argv: Vec<_> = args.into_iter()
        .map(|x| x.into_raw())
        .collect();
    argv.push(std::ptr::null_mut());
    (unsafe { Py_Main((argv.len() - 1) as c_int, argv.as_mut_ptr()) }) as i32
}

pub fn py_set_python_home<P: AsRef<Path>>(path: &P) {
    let path = path_to_local_cstring(path.as_ref());
    unsafe { Py_SetPythonHome(path.into_raw()) };
}
