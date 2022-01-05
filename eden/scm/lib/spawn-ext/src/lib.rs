/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This crate extends the `process::Command` interface in stdlib.
//! - `avoid_inherit_handles` is similar to `close_fds=True` in Python.
//! - `new_session` uses `CREATE_NEW_PROCESS_GROUP` on Windows, and `setsid` on
//!   Unix.
//! - `spawn_detached` is a quicker way to spawn and forget.

use std::io;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;

/// Extensions to `std::process::Command`.
pub trait CommandExt {
    /// Attempt to avoid inheriting file handles.
    /// Call this before setting up redirections!
    fn avoid_inherit_handles(&mut self) -> &mut Self;

    /// Use a new session for the new process.
    /// Call this after `avoid_inherit_handles`!
    fn new_session(&mut self) -> &mut Self;

    /// Spawn a process with stdio redirected to null and forget about it.
    /// Return the process id.
    fn spawn_detached(&mut self) -> io::Result<Child>;
}

impl CommandExt for Command {
    fn avoid_inherit_handles(&mut self) -> &mut Self {
        #[cfg(unix)]
        unix::avoid_inherit_handles(self);

        #[cfg(windows)]
        windows::avoid_inherit_handles(self);

        self
    }

    fn new_session(&mut self) -> &mut Self {
        #[cfg(unix)]
        unix::new_session(self);

        #[cfg(windows)]
        windows::new_session(self);

        self
    }

    fn spawn_detached(&mut self) -> io::Result<Child> {
        self.avoid_inherit_handles()
            .new_session()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    }
}

#[cfg(windows)]
mod windows {
    use std::os::windows::process::CommandExt;

    use winapi::um::handleapi::SetHandleInformation;
    use winapi::um::winbase::CREATE_NEW_PROCESS_GROUP;
    use winapi::um::winbase::CREATE_NO_WINDOW;
    use winapi::um::winbase::HANDLE_FLAG_INHERIT;

    use super::*;

    // A larger value like 8192 adds visible overhead (ex. >5ms).
    // At first we had 2048, however it wasn't enough in some cases
    // https://fburl.com/px16lb62. We bumped it to 4096.
    const MAX_HANDLE: usize = 4096;

    pub fn avoid_inherit_handles(command: &mut Command) {
        // Attempt to mark handles as "not inheritable".
        //
        // Practically only a few handles are accidentally inheritable.
        // Use Process Hacker to examine handles [1]. Inheritable handles
        // are highlighted in cyan background. Example:
        //
        //  File, \Device\ConDrv, 0x58
        //  File, \Device\ConDrv, 0x5c
        //  File, \Device\ConDrv, 0x60
        //  File, \Device\Afd, 0x348
        //
        // [1]: https://github.com/processhacker/processhacker/
        for handle in 1..=MAX_HANDLE {
            // According to https://devblogs.microsoft.com/oldnewthing/20050121-00/?p=36633
            // kernel handles are always a multiple of 4
            let handle = unsafe { std::mem::transmute(handle * 4) };
            unsafe {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0)
            };
        }
        // A cleaner way might be setting bInheritHandles to FALSE at
        // CreateProcessW time. However the Rust stdlib does not expose an
        // interface to set bInheritHandles, and bInheritHandles=FALSE
        // could break file redirections (with possible solutions [2] [3]).
        //
        // [2]: https://github.com/python/cpython/commit/b2a6083eb0384f38839d
        // [3]: https://devblogs.microsoft.com/oldnewthing/20111216-00/?p=8873

        // CREATE_NO_WINDOW forbids allocating stdio handles.
        command.creation_flags(CREATE_NO_WINDOW);
    }

    pub fn new_session(command: &mut Command) {
        command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
}

#[cfg(unix)]
mod unix {
    use std::os::unix::process::CommandExt;

    use super::*;

    // Linux by default has max fd limited to 1024.
    // 2048 is practically more than enough with about 2ms overhead.
    const MAXFD: i32 = 2048;

    pub fn avoid_inherit_handles(command: &mut Command) {
        // There are some constraints for this function.
        // See std::os::unix::process::CommandExt::pre_exec.
        // Namely, do not allocate.
        unsafe {
            command.pre_exec(pre_exec_close_fds)
        };
    }

    pub fn new_session(command: &mut Command) {
        unsafe {
            command.pre_exec(pre_exec_setsid)
        };
    }

    fn pre_exec_close_fds() -> io::Result<()> {
        // Set FD_CLOEXEC on files.
        // Note: using `close` might break error reporting if exec fails.
        for fd in 3..=MAXFD {
            unsafe {
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC)
            };
        }
        Ok(())
    }

    fn pre_exec_setsid() -> io::Result<()> {
        // Create a new session.
        unsafe {
            libc::setsid()
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // It's hard to test the real effects. Here we just check command still runs.
    // Use `cargo run --example spawn` to manually check the close_fds behavior.
    #[test]
    fn smoke_test_command_still_runs() {
        let dir = tempfile::tempdir().unwrap();

        let args = if cfg!(unix) {
            vec!["/bin/sh", "-c", "echo foo > a"]
        } else {
            vec!["cmd.exe", "/c", "echo foo > a"]
        };
        let mut command = if cfg!(unix) {
            Command::new(&args[0])
        } else {
            Command::new(&args[0])
        };
        let mut child = command
            .args(&args[1..])
            .current_dir(&dir.path())
            .spawn_detached()
            .unwrap();
        child.wait().unwrap();

        assert_eq!(&std::fs::read(dir.path().join("a")).unwrap()[..3], b"foo")
    }
}
