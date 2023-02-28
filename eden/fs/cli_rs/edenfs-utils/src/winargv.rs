/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg(windows)]

use std::borrow::Cow;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::ops::Deref;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::ffi::OsStringExt;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::shellapi::CommandLineToArgvW;
use winapi::um::winbase::LocalFree;

/// Quotes (if necessary) a set of Windows command-line arguments for consumption
/// by CommandLineToArgvW.
///
/// N.B.: This does not perform quoting for cmd.exe.
pub fn argv_to_command_line(argv: &[&OsStr]) -> Result<OsString> {
    let quoted_args = argv
        .iter()
        .enumerate()
        .map(|(i, a)| {
            if i == 0 && has_illegal_cmd_chars(&a) {
                bail!("illegal chars for argv[0] in {:?}", a);
            }
            quote_arg(a)
        })
        .collect::<Result<Vec<_>>>()?;

    let mut result = OsString::new();
    for (i, a) in quoted_args.into_iter().enumerate() {
        if i != 0 {
            result.push(" ");
        }
        result.push(a.deref());
    }
    Ok(result)
}

fn has_illegal_cmd_chars(cmd: &OsStr) -> bool {
    match cmd.to_str() {
        Some(cmd_str) => cmd_str.contains('"') || cmd_str.ends_with('\\'),
        None => true,
    }
}

/// Quotes a single argument based on the algorithm described by Microsoft:
/// https://learn.microsoft.com/en-us/archive/blogs/twistylittlepassagesallalike/everyone-quotes-command-line-arguments-the-wrong-way
///
/// With the modification that we bail out if we're asked to quote control
/// characters, since CommandLineToArgvW can't round-trip them.
fn quote_arg<'a>(arg: &'a OsStr) -> Result<Cow<'a, OsStr>> {
    let arg_str = arg
        .to_str()
        .ok_or(anyhow!("arg invalid as Unicode string"))?;

    for c in arg_str.chars() {
        if c.is_control() {
            bail!("cannot quote control characters");
        }
    }

    const CHARS_TO_QUOTE: [char; 5] = [' ', '\t', '\n', '\u{000B}', '"'];
    if arg_str.find(&CHARS_TO_QUOTE) == None && !arg_str.is_empty() {
        // No quoting needed.
        return Ok(Cow::Borrowed(arg));
    }

    let mut quoted = "\"".to_owned();
    let mut num_backslashes = 0usize;
    for c in arg_str.chars() {
        if c == '\\' {
            num_backslashes += 1;
            continue;
        }
        if c == '"' {
            push_n(&mut quoted, 2 * num_backslashes + 1, '\\');
        } else {
            push_n(&mut quoted, num_backslashes, '\\');
        }
        quoted.push(c);
        num_backslashes = 0;
    }
    push_n(&mut quoted, 2 * num_backslashes, '\\');
    quoted.push('"');

    Ok(Cow::Owned(quoted.into()))
}

fn push_n(s: &mut String, n: usize, c: char) {
    for _ in 0..n {
        s.push(c);
    }
}

/// Parses a Windows command line into an argv vector of program name followed
/// by zero or more command-line arguments.
pub fn command_line_to_argv(command_line: &OsStr) -> Result<Vec<OsString>> {
    if command_line.is_empty() {
        // CommandLineToArgvW assumes the current executable file if passed an
        // empty string, but we don't want that behavior.
        return Ok(vec![]);
    }

    let command_line_w = command_line
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();

    let mut argv = Vec::<OsString>::new();

    let mut num_args: i32 = 0;
    let argv_w = LocalPtr(unsafe { CommandLineToArgvW(command_line_w.as_ptr(), &mut num_args) });
    if argv_w.0.is_null() {
        return Err(anyhow!("CommandLineToArgvW failed: {:?}", unsafe {
            GetLastError()
        }));
    }

    for i in 0..num_args {
        let arg_offset: isize = i.try_into()?;
        let arg_w = unsafe { *argv_w.0.offset(arg_offset) } as *const u16;
        let arg_w_slice = unsafe { null_terminated_slice(arg_w)? };
        argv.push(OsString::from_wide(arg_w_slice));
    }
    Ok(argv)
}

/// Owned pointer that needs to be freed with LocalFree.
struct LocalPtr<T>(*mut T);

impl<T> Drop for LocalPtr<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0 as *mut _) };
        }
    }
}

/// Given a pointer to a null-terminated wide character string, returns a slice
/// of the string, excluding the null wide character.  Behavior is undefined if
/// given a pointer to a non-null-terminated string.
unsafe fn null_terminated_slice<'a>(ptr: *const u16) -> Result<&'a [u16]> {
    let mut i = 0isize;
    loop {
        if *ptr.offset(i) == 0u16 {
            break;
        }
        i += 1;
    }
    if *ptr.offset(i) != 0u16 {
        bail!("No null terminator found");
    }

    let slice_size: usize = i.try_into()?;
    Ok(std::slice::from_raw_parts(ptr, slice_size))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::ffi::OsString;
    use std::str::FromStr;

    use anyhow::Result;
    use quickcheck::TestResult;
    use quickcheck_macros::quickcheck;

    use super::argv_to_command_line;
    use super::command_line_to_argv;
    use super::null_terminated_slice;
    use super::quote_arg;

    #[test]
    fn test_quote_arg() -> Result<()> {
        assert_eq!(quote_arg(OsStr::new(""))?, OsString::from_str("\"\"")?);
        assert_eq!(
            quote_arg(OsStr::new("argument1"))?,
            OsString::from_str("argument1")?
        );
        assert_eq!(
            quote_arg(OsStr::new("argument 2"))?,
            OsString::from_str("\"argument 2\"")?
        );
        assert_eq!(
            quote_arg(OsStr::new("\\some\\path with\\spaces"))?,
            OsString::from_str("\"\\some\\path with\\spaces\"")?
        );
        assert_eq!(
            quote_arg(OsStr::new("\\some\\path\\without\\spaces"))?,
            OsString::from_str("\\some\\path\\without\\spaces")?
        );
        assert_eq!(
            quote_arg(OsStr::new("with\"quote"))?,
            OsString::from_str("\"with\\\"quote\"")?
        );

        Ok(())
    }

    fn argv_round_trips(argv: Vec<OsString>) -> Result<bool> {
        let quoted_argv = argv_to_command_line(
            argv.iter()
                .map(OsString::as_os_str)
                .collect::<Vec<_>>()
                .as_slice(),
        )?;

        let parsed_argv = command_line_to_argv(quoted_argv.as_os_str()).unwrap();
        Ok(parsed_argv == argv)
    }

    // Try to gain confidence that our argv_to_command_line round-trips with
    // win32's CommandLineToArgvW, or returns an error if quoting is impossible.
    // The important cases to cover seem to be quoting argv[0] and argv[1..],
    // so we cover those two cases explicitly to allow quickcheck to generate
    // more comprehensive sets of test strings.
    #[quickcheck]
    fn check_argv_to_command_line_round_trips_argv0(argv: OsString) -> TestResult {
        let round_trips_ok = argv_round_trips(vec![argv]);
        if round_trips_ok.is_err() {
            // Discard arguments that quote_arg recognizes as non-quotable.
            return TestResult::discard();
        }
        TestResult::from_bool(round_trips_ok.unwrap())
    }

    #[quickcheck]
    fn check_argv_to_command_line_round_trips_argv1(argv: OsString) -> TestResult {
        let round_trips_ok = argv_round_trips(vec![OsString::from_str("foo.exe").unwrap(), argv]);
        if round_trips_ok.is_err() {
            // Discard arguments that quote_arg recognizes as non-quotable.
            return TestResult::discard();
        }
        TestResult::from_bool(round_trips_ok.unwrap())
    }

    // Specific round trip cases to check.
    #[test]
    fn test_argv_to_command_line_round_trips() -> Result<()> {
        assert!(argv_round_trips(vec![OsString::from("\"")]).is_err());
        assert!(argv_round_trips(vec![OsString::from(" \\")]).is_err());
        assert!(argv_round_trips(vec![OsString::from(" !\\")]).is_err());
        assert!(argv_round_trips(vec![OsString::from(" \\a")])?);

        Ok(())
    }

    #[test]
    fn test_command_line_to_argv() -> Result<()> {
        assert!(command_line_to_argv(OsStr::new(""))?.is_empty());
        assert_eq!(
            command_line_to_argv(OsStr::new("C:\\Windows\\system32\\svchost.exe"))?,
            vec![OsString::from_str("C:\\Windows\\system32\\svchost.exe")?]
        );
        assert_eq!(
            command_line_to_argv(OsStr::new("foo.exe bar.txt"))?,
            vec![
                OsString::from_str("foo.exe")?,
                OsString::from_str("bar.txt")?
            ]
        );

        Ok(())
    }

    #[test]
    fn test_null_terminated_slice() -> Result<()> {
        unsafe {
            assert!(null_terminated_slice(vec![0u16].as_ptr())?.is_empty());
            assert_eq!(
                null_terminated_slice(vec![1u16, 0u16].as_ptr())?,
                vec![1u16]
            );
            assert_eq!(
                null_terminated_slice(vec![1u16, 2u16, 0u16].as_ptr())?,
                vec![1u16, 2u16]
            );
        }

        Ok(())
    }
}
