// Copyright Facebook, Inc. 2018
use encoding::osstring_to_local_cstring;
use libc::{c_char, c_int};
use std::ffi::{CString, OsString};
use std::path::Path;

#[cfg_attr(not(fbcode_build), link(name = "chg", kind = "static"))]
extern "C" {
    fn chg_main(argc: c_int, argv: *mut *mut c_char, envp: *mut *mut c_char) -> c_int;
}

/// Call `chg_main` with given environment and arguments
fn chg_main_wrapper(args: Vec<CString>, envs: Vec<CString>) -> i32 {
    let mut argv: Vec<_> = args.into_iter().map(|x| x.into_raw()).collect();
    argv.push(std::ptr::null_mut());
    let mut envp: Vec<_> = envs.into_iter().map(|x| x.into_raw()).collect();
    envp.push(std::ptr::null_mut());
    let rc = unsafe {
        chg_main(
            (argv.len() - 1) as c_int,
            argv.as_mut_ptr(),
            envp.as_mut_ptr(),
        )
    } as i32;
    rc
}

/// Turn `OsString` args into `CString` for ffi
/// For now, this is just copied from the `hgcommands`
/// crate, but in future this should be a part
/// of `argparse` crate
fn args_to_local_cstrings() -> Vec<CString> {
    std::env::args_os()
        .map(|x| osstring_to_local_cstring(&x))
        .collect()
}

/// Turn `OsString` pairs from `vars_os`
/// into `name=value` `CString`s, suitable
/// to be passed as `envp` to `chg_main`
fn env_to_local_cstrings() -> Vec<CString> {
    std::env::set_var("CHGHG", std::env::current_exe().unwrap());
    std::env::vars_os()
        .map(|(name, value)| {
            let mut envstr = OsString::new();
            envstr.push(name);
            envstr.push("=");
            envstr.push(value);
            osstring_to_local_cstring(&envstr).clone()
        })
        .collect()
}

/// Make decision based on a file `path`
/// - `None` if file does not exist
/// - `Some(true)` if file contains 1
/// - `Some(false)` otherwise
fn file_decision(path: Option<impl AsRef<Path>>) -> Option<bool> {
    path.and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|s| s.starts_with("1"))
}

/// Checks if chg should be used to execute a command
/// TODO: implement command-based filtering logic
///       which would provide us with command names
///       to always skip
fn should_call_chg(args: &Vec<String>) -> bool {
    if cfg!(target_os = "windows") {
        return false;
    }
    // This means we're already inside the chg call chain
    if std::env::var_os("CHGINTERNALMARK").is_some() {
        return false;
    }

    if let Some(arg) = args.get(1) {
        if arg == "debugpython" {
            // debugpython is incompatible with chg.
            return false;
        }
    }

    // CHGDISABLE=1 means that we want to disable it
    // regardless of the other conditions, but CHGDISABLE=0
    // does not guarantee that we want to enable it
    if Some(OsString::from("1")) == std::env::var_os("CHGDISABLE") {
        return false;
    }

    if let Some(home_decision) = file_decision(dirs::home_dir().map(|d| d.join(".usechg"))) {
        return home_decision;
    }

    if let Some(etc_decision) = file_decision(Some("/etc/mercurial/usechg")) {
        return etc_decision;
    }

    return false;
}

/// Perform needed checks and maybe pass control to chg
/// Note that this function terminates the process
/// if it decides to pass control to chg
pub fn maybe_call_chg(args: &Vec<String>) {
    if !should_call_chg(args) {
        return;
    }
    let rc = chg_main_wrapper(args_to_local_cstrings(), env_to_local_cstrings());
    std::process::exit(rc);
}
