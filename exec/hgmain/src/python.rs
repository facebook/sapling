use encoding::path_to_local_bytes;
use libc::c_int;
use python27_sys::{Py_Main, Py_SetPythonHome};
use std;
use std::ffi::CString;
use std::path::Path;

pub fn py_main(args: Vec<Vec<u8>>) -> i32 {
    let mut argv: Vec<_> = args.into_iter()
        .map(|x| CString::new(x).unwrap().into_raw())
        .collect();
    argv.push(std::ptr::null_mut());
    (unsafe { Py_Main((argv.len() - 1) as c_int, argv.as_mut_ptr()) }) as i32
}

pub fn py_set_python_home<T: AsRef<Path>>(path: T) {
    let path = CString::new(path_to_local_bytes(path.as_ref()).unwrap()).unwrap();
    unsafe { Py_SetPythonHome(path.into_raw()) };
}
