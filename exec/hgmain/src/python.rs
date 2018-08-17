use libc::c_int;
use local_encoding::{Encoder, Encoding};
use python27_sys::{Py_Main, Py_SetPythonHome};
use std;
use std::borrow::ToOwned;
use std::ffi::CString;
use std::path::{Path, PathBuf};

pub fn py_main(args: Vec<CString>) -> i32 {
    let mut argv: Vec<_> = args.into_iter().map(|x| x.into_raw()).collect();
    argv.push(std::ptr::null_mut());
    (unsafe { Py_Main((argv.len() - 1) as c_int, argv.as_mut_ptr()) }) as i32
}

pub fn py_set_python_home<T: AsRef<Path>>(path: T) {
    let path: PathBuf = path.as_ref().to_owned();
    let path = path.to_str()
        .expect("could not convert the desired python home");
    let path = (Encoding::ANSI).to_bytes(path).unwrap();
    let path = CString::new(path).unwrap();
    unsafe { Py_SetPythonHome(path.into_raw()) };
}
