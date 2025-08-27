/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ffi::CString;
use std::ffi::OsString;
use std::path::Path;
use std::time::Instant;

use clidispatch::io::IsTty;
use configmodel::ConfigExt;
use encoding::osstring_to_local_cstring;
use libc::c_char;
use libc::c_int;

unsafe extern "C" {
    fn chg_main(
        argc: c_int,
        argv: *mut *mut c_char,
        envp: *mut *mut c_char,
        cli_name: *const c_char,
        versionhash: u64,
    ) -> c_int;
}

/// Call `chg_main` with given environment and arguments
fn chg_main_wrapper(args: Vec<CString>, envs: Vec<CString>) -> i32 {
    let mut argv: Vec<_> = args.into_iter().map(|x| x.into_raw()).collect();
    argv.push(std::ptr::null_mut());
    let mut envp: Vec<_> = envs.into_iter().map(|x| x.into_raw()).collect();
    envp.push(std::ptr::null_mut());
    let name = identity::default().cli_name();
    let name = CString::new(name).unwrap();
    const VERSION_HASH_INT: u64 = match u64::from_str_radix(::version::VERSION_HASH, 10) {
        Err(_) => 0,
        Ok(v) => v,
    };

    unsafe {
        chg_main(
            (argv.len() - 1) as c_int,
            argv.as_mut_ptr(),
            envp.as_mut_ptr(),
            name.as_c_str().as_ptr(),
            VERSION_HASH_INT,
        )
    }
}

/// Turn `OsString` args into `CString` for ffi
/// For now, this is just copied from the `commands`
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
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("CHGHG", std::env::current_exe().unwrap()) };
    std::env::vars_os()
        .map(|(name, value)| {
            let mut envstr = OsString::new();
            envstr.push(name);
            envstr.push("=");
            envstr.push(value);
            osstring_to_local_cstring(&envstr)
        })
        .collect()
}

/// Make decision based on a file `path`
/// - `None` if file does not exist
/// - `Some(true)` if file contains 1
/// - `Some(false)` otherwise
fn file_decision(path: Option<impl AsRef<Path>>) -> Option<bool> {
    path.and_then(|p| std::fs::read(p).ok())
        .map(|bytes| bytes.starts_with(b"1"))
}

/// Checks if chg should be used to execute a command
/// TODO: implement command-based filtering logic
///       which would provide us with command names
///       to always skip
fn should_call_chg(args: &[String]) -> (bool, &'static str) {
    // First check conditions we _never_ want to run chg.

    if cfg!(target_os = "windows") {
        return (false, "windows");
    }

    // This means we're already inside the chg call chain
    if std::env::var_os("CHGINTERNALMARK").is_some() {
        return (false, "CHGINTERNALMARK");
    }

    // debugpython is incompatible with chg.
    if args.get(1).is_some_and(|x| x == "debugpython") {
        return (false, "debugpython");
    }

    // stdin is not a tty but stdout is a tty. Interactive pager is used
    // but lack of ctty makes it impossible to control the interactive
    // pager via keys.
    if cfg!(unix) && !std::io::stdin().is_tty() && std::io::stdout().is_tty() {
        return (false, "!stdin.is_tty() && stdout.is_tty()");
    }

    // Bash might translate `<(...)` to `/dev/fd/x` instead of using a real fifo. That
    // path resolves to different fd by the chg server. Therefore chg cannot be used.
    if cfg!(unix)
        && args
            .iter()
            .any(|a| a.starts_with("/dev/fd/") || a.starts_with("/proc/self/"))
    {
        return (false, "arg starts with /dev/fd|/proc/self/");
    }

    // Now check CHGDISABLE. We check this first since it allows us to force enablement (using CHGDISABLE=never).

    // CHGDISABLE=1 means that we want to disable it
    // regardless of the other conditions, but CHGDISABLE=0
    // does not guarantee that we want to enable it. CHGDISABLE=never
    // means we want to enable it, overriding the below file decisions.
    if let Some(val) = std::env::var_os("CHGDISABLE") {
        if val == "never" {
            return (true, "CHGDISABLE=never");
        }
        if val == "1" {
            return (false, "CHGDISABLE=1");
        }
    }

    if !cfg!(feature = "fb") && cfg!(target_os = "macos") {
        return (false, "macos");
    }

    // do not use chg in dev build, unless in tests
    if ::version::VERSION.ends_with("dev") && std::env::var_os("TESTTMP").is_none() {
        return (false, "dev");
    }

    if cfg!(fbcode_build) {
        if let Some(home_decision) = file_decision(dirs::home_dir().map(|d| d.join(".usechg"))) {
            return (home_decision, "~/.usechg");
        }

        if let Some(etc_decision) = file_decision(Some("/etc/mercurial/usechg")) {
            return (etc_decision, "/etc/mercurial/usechg");
        }
    } else if cfg!(unix) {
        return (true, "unix");
    }

    (false, "(default fallthrough)")
}

/// Perform needed checks and maybe pass control to chg
/// Note that this function terminates the process
/// if it decides to pass control to chg
pub fn maybe_call_chg(args: &[String]) {
    let (mut should_use, mut reason) = should_call_chg(args);

    let chgdebug = std::env::var_os("CHGDEBUG").is_some_and(|x| x == "1");

    if should_use {
        let start_time = Instant::now();
        match configloader::hg::local_load(configloader::hg::RepoInfo::NoRepo, &[]) {
            Ok(config) => {
                if config.must_get("chg", "disable").unwrap_or(false) {
                    should_use = false;
                    reason = "chg.disable=true in config";
                } else if cfg!(target_os = "linux") {
                    if let Ok(cgroup_regex) =
                        config.must_get::<configmodel::Regex>("chg", "cgroup-regex")
                    {
                        if let Ok(my_cgroup) = std::fs::read_to_string("/proc/self/cgroup") {
                            let my_cgroup = my_cgroup.trim();
                            if chgdebug {
                                eprintln!(
                                    "chg: debug: my cgroup: {my_cgroup}, cgroup regex: {}",
                                    cgroup_regex.as_str()
                                );
                            }
                            if !cgroup_regex.is_match(my_cgroup) {
                                should_use = false;
                                reason = "chg.cgroup-regex set and doesn't match /proc/self/cgroup";
                            }
                        }
                    }
                }
            }
            Err(err) => {
                if chgdebug {
                    eprintln!(
                        "chg: debug: error loading config: {}",
                        format!("{err:?}").trim()
                    );
                }
                should_use = false;
                reason = "error loading config";
            }
        }
        if chgdebug {
            eprintln!(
                "chg: debug: config based decision took {:?}",
                start_time.elapsed()
            );
        }
    }

    if chgdebug {
        eprintln!("chg: debug: using chg: {}, because {}", should_use, reason);
    }

    if !should_use {
        return;
    }

    let rc = chg_main_wrapper(args_to_local_cstrings(), env_to_local_cstrings());
    std::process::exit(rc);
}
