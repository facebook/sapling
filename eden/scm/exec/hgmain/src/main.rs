/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clidispatch::dispatch;

mod buildinfo;
#[cfg(all(feature = "with_chg", not(windows)))]
mod chg;
#[cfg(all(feature = "with_chg", not(windows)))]
use chg::maybe_call_chg;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::disable_standard_handle_inheritability;
#[cfg(windows)]
use windows::is_edenfs_stopped;

#[cfg_attr(fbcode_build, fbinit::main)]
fn main() {
    // Allow dropping root, which is useful when running under DTrace on Mac.
    #[cfg(unix)]
    if let Ok(Some((user, group))) = std::env::var("SL_DEBUG_DROP_ROOT")
        .as_ref()
        .map(|u| u.split_once(':'))
    {
        drop_root(user, group);
    }

    if cfg!(windows) {
        // Setting NoDefaultCurrentDirectoryInExePath disables the
        // default Windows behavior of including the current working
        // directory in $PATH. This avoids a class of security issues
        // where we might accidentally prefer an executable (such as
        // "watchman") from the working copy.
        std::env::set_var("NoDefaultCurrentDirectoryInExePath", "1")
    }

    let mut full_args = match dispatch::args() {
        Ok(args) => args,
        Err(_) => {
            eprintln!("abort: cannot decode command line arguments");
            std::process::exit(255);
        }
    };

    // On Windows we need to check if the repository is backed by EdenFS and abort if it is not
    // running to avoid writing any files that may bring the repository into a bad state.
    #[cfg(windows)]
    {
        if let Ok(cwd) = std::env::current_dir() {
            if is_edenfs_stopped(&cwd) {
                eprintln!(
                    "error: repository is not mounted. Check if EdenFS is running and ensure repository is mounted.\nhint: try `edenfsctl doctor`."
                );
                std::process::exit(255);
            }
        }
    }

    match full_args.get(0).map(AsRef::as_ref) {
        Some("buildinfo") => {
            // This code path keeps buildinfo-related symbols alive.
            #[cfg(feature = "buildinfo")]
            unsafe {
                buildinfo::print_buildinfo();
            }

            #[cfg(not(feature = "buildinfo"))]
            {
                eprintln!("buildinfo not compiled in!");
            }

            return;
        }
        Some(name) => {
            if name.ends_with("python") || name.ends_with("python3") {
                // Translate to the "debugpython" command.
                // ex. "python foo.py" => "hg debugpython -- foo.py"
                let debugpython_args = vec!["hg", "debugpython", "--"]
                    .into_iter()
                    .map(ToString::to_string)
                    .chain(full_args.into_iter().skip(1))
                    .collect::<Vec<String>>();
                full_args = debugpython_args;
            }
        }
        _ => {}
    }

    #[cfg(all(feature = "with_chg", not(windows)))]
    maybe_call_chg(&full_args);

    #[cfg(windows)]
    disable_standard_handle_inheritability().unwrap();

    #[cfg(windows)]
    windows::enable_vt_processing().unwrap();

    let mut io = clidispatch::io::IO::stdio();

    let _ = io.setup_term();

    io.set_main();
    let mut code = hgcommands::run_command(full_args, &mut io);
    if io.flush().is_err() {
        if code == 0 {
            code = 255;
        }
    }
    drop(io);
    std::process::exit(code as i32);
}

#[cfg(unix)]
pub fn drop_root(user: &str, group: &str) {
    use std::ffi::CStr;
    use std::ffi::CString;

    let cgroup = CString::new(group.as_bytes()).unwrap();
    let libc_group = unsafe { libc::getgrnam(cgroup.as_ptr()) };
    if libc_group.is_null() {
        panic!("bad group '{}'", group);
    }
    if unsafe { libc::setgid((*libc_group).gr_gid) } != 0 {
        panic!(
            "failed setting group to '{}': {:?}",
            group,
            std::io::Error::last_os_error()
        );
    }

    let cuser = CString::new(user.as_bytes()).unwrap();
    let libc_user = unsafe { libc::getpwnam(cuser.as_ptr()) };
    if libc_user.is_null() {
        panic!("bad user '{}'", user);
    }
    if unsafe { libc::setuid((*libc_user).pw_uid) } != 0 {
        panic!(
            "failed setting user to '{}': {:?}",
            user,
            std::io::Error::last_os_error()
        );
    }

    // Set $HOME and $USER for convenience since various things depend on those.
    let home_dir = unsafe { (*libc_user).pw_dir };
    if !home_dir.is_null() {
        std::env::set_var(
            "HOME",
            unsafe { CStr::from_ptr(home_dir) }.to_str().unwrap(),
        );
    }
    std::env::set_var("USER", user);

    eprintln!("switched user/group to {}/{}", user, group);
}
