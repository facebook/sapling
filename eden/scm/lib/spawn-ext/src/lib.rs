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

use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::process::Child;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Output;
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

    /// Similar to `Output` but reports as an error for non-zero exit code.
    fn checked_output(&mut self) -> io::Result<Output>;

    /// Similar to `status` but reports an error for non-zero exits.
    fn checked_run(&mut self) -> io::Result<ExitStatus>;

    /// Create a `Command` to run `shell_cmd` through system's shell. This uses "cmd.exe"
    /// on Windows and "/bin/sh" otherwise. Do not add more args to the returned
    /// `Command`. On Windows, you do not need to use the shell to run batch files (the
    /// Rust stdlib detects batch files and uses "cmd.exe" automatically).
    fn new_shell(shell_cmd: impl AsRef<str>) -> Command;
}

#[derive(Debug)]
struct CommandError {
    title: String,
    command: String,
    output: String,
    source: Option<io::Error>,
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // title
        //   command
        //     output
        let title = if self.title.is_empty() {
            "CommandError:"
        } else {
            self.title.as_str()
        };
        write!(f, "{}\n  {}\n", title, &self.command)?;
        for line in self.output.lines() {
            write!(f, "    {line}\n")?;
        }
        Ok(())
    }
}

impl Error for CommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|s| s as &dyn Error)
    }
}

fn os_str_to_naive_quoted_str(s: &OsStr) -> String {
    let debug_format = format!("{:?}", s);
    if debug_format.len() == s.len() + 2
        && debug_format.split_ascii_whitespace().take(2).count() == 1
    {
        debug_format[1..debug_format.len() - 1].to_string()
    } else {
        debug_format
    }
}

impl CommandError {
    fn new(command: &Command, source: Option<io::Error>) -> Self {
        let arg0 = os_str_to_naive_quoted_str(command.get_program());
        let args = command
            .get_args()
            .map(os_str_to_naive_quoted_str)
            .collect::<Vec<String>>()
            .join(" ");
        let command = format!("{arg0} {args}");
        Self {
            title: Default::default(),
            output: Default::default(),
            command,
            source,
        }
    }

    fn with_output(mut self, output: &Output) -> Self {
        for out in [&output.stdout, &output.stderr] {
            self.output.push_str(&String::from_utf8_lossy(out));
        }
        self.with_status(&output.status)
    }

    fn with_status(mut self, exit: &ExitStatus) -> Self {
        match exit.code() {
            None =>
            {
                #[cfg(unix)]
                match std::os::unix::process::ExitStatusExt::signal(exit) {
                    Some(sig) => self.title = format!("Command terminated by signal {}", sig),
                    None => {}
                }
            }
            Some(code) => {
                if code != 0 {
                    self.title = format!("Command exited with code {}", code);
                }
            }
        }
        self
    }

    fn into_io_error(self: CommandError) -> io::Error {
        let kind = match self.source.as_ref() {
            None => io::ErrorKind::Other,
            Some(e) => e.kind(),
        };
        io::Error::new(kind, self)
    }
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

    fn checked_output(&mut self) -> io::Result<Output> {
        let out = self
            .output()
            .map_err(|e| CommandError::new(self, Some(e)).into_io_error())?;
        if !out.status.success() {
            return Err(CommandError::new(self, None)
                .with_output(&out)
                .into_io_error());
        }
        Ok(out)
    }

    fn checked_run(&mut self) -> io::Result<ExitStatus> {
        let status = self
            .status()
            .map_err(|e| CommandError::new(self, Some(e)).into_io_error())?;
        if !status.success() {
            return Err(CommandError::new(self, None)
                .with_status(&status)
                .into_io_error());
        }
        Ok(status)
    }

    fn new_shell(shell_cmd: impl AsRef<str>) -> Command {
        #[cfg(unix)]
        return unix::new_shell(shell_cmd);

        #[cfg(windows)]
        return windows::new_shell(shell_cmd);
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::process::CommandExt;

    use winapi::shared::minwindef::DWORD;
    use winapi::shared::minwindef::MAX_PATH;
    use winapi::um::handleapi::SetHandleInformation;
    use winapi::um::sysinfoapi::GetSystemDirectoryW;
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
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
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

    pub fn new_shell(shell_cmd: impl AsRef<str>) -> Command {
        // This essentially prepares a command with ["C:\Windows\system32\cmd.exe", "/c"];

        let cmd_exe = match std::env::var_os("ComSpec") {
            Some(val) => val,
            None => {
                tracing::info!("$ComSpec isn't set");
                // Python does join($env:SystemRoot, "System32", "cmd.exe"), but calling
                // GetSystemDirectoryW seems a little better.

                let mut cmd_exe = vec![0u16; MAX_PATH];
                let ret =
                    unsafe { GetSystemDirectoryW(cmd_exe.as_mut_ptr(), cmd_exe.len() as DWORD) };
                if ret == 0 {
                    tracing::warn!("GetSystemDirectoryW didn't work");

                    // Shouldn't happen, use hard coded fallback just-in-case. Avoid plain
                    // "cmd.exe" since that is susceptible to the CWD-is-in-PATH behavior.
                    r"C:\Windows\system32\cmd.exe".to_string().into()
                } else {
                    // Truncate before NULL (i.e. `ret` is length without trailing NULL).
                    cmd_exe.truncate(ret as usize);

                    // Append "\cmd.exe".
                    cmd_exe.extend(r"\cmd.exe".encode_utf16());

                    OsString::from_wide(&cmd_exe)
                }
            }
        };

        let mut cmd = Command::new(cmd_exe);

        cmd.arg("/c");

        let shell_cmd = shell_cmd.as_ref();

        // Rust's standard library "auto quote" behavior on Windows does not work w/ cmd.exe.
        // It turns `"C:\foo bar.exe" baz` into `"\"C:\foo bar.exe\" baz"`, which fails.
        // So, quote the cmd ourselves and use raw_arg to disable std lib quoting.
        if need_cmd_quote(shell_cmd) {
            // Unlike typical shell quoting, we don't escape double quotes within.
            // This is what cmd.exe expects.
            cmd.raw_arg(format!("\"{}\"", shell_cmd));
        } else {
            cmd.raw_arg(shell_cmd);
        }

        cmd
    }

    fn need_cmd_quote(cmd: &str) -> bool {
        // Work with D49694880 - should never triple quote.
        if cmd.starts_with("\"\"") && cmd.ends_with("\"\"") {
            return false;
        }
        true
    }

    #[cfg(test)]
    #[test]
    fn test_need_cmd_quote() {
        assert!(need_cmd_quote("foo bar"));
        assert!(need_cmd_quote("\"foo bar\""));
        assert!(need_cmd_quote("\"foo\" \"bar\""));
        assert!(!need_cmd_quote("\"\"foo bar\"\""));
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
        unsafe { command.pre_exec(pre_exec_close_fds) };
    }

    pub fn new_session(command: &mut Command) {
        unsafe { command.pre_exec(pre_exec_setsid) };
    }

    fn pre_exec_close_fds() -> io::Result<()> {
        // Set FD_CLOEXEC on files.
        // Note: using `close` might break error reporting if exec fails.
        for fd in 3..=MAXFD {
            unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
        }
        Ok(())
    }

    fn pre_exec_setsid() -> io::Result<()> {
        // Create a new session.
        unsafe { libc::setsid() };
        Ok(())
    }

    pub fn new_shell(shell_cmd: impl AsRef<str>) -> Command {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg(shell_cmd.as_ref());
        cmd
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
            Command::new(args[0])
        } else {
            Command::new(args[0])
        };
        let mut child = command
            .args(&args[1..])
            .current_dir(dir.path())
            .spawn_detached()
            .unwrap();
        child.wait().unwrap();

        assert_eq!(&std::fs::read(dir.path().join("a")).unwrap()[..3], b"foo")
    }

    #[test]
    fn test_shell() {
        let stdout = Command::new_shell("echo foo").output().unwrap().stdout;

        if cfg!(windows) {
            assert_eq!(stdout, b"foo\r\n");
        } else {
            assert_eq!(stdout, b"foo\n");
        }
    }
}
