/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::process::Command;

/// Return a `Command` intended to run a "system" command.
pub fn new_system_command(command: String) -> Command {
    // Maybe we don't actually need a shell.
    let need_shell = command.contains(|ch| "|&;<>()$`\"' \t\n*?[#~=%".contains(ch)) || {
        let path = Path::new(&command);
        path.is_absolute() && !path.exists()
    };

    if need_shell {
        let mut cmd = if cfg!(windows) {
            let cmd_spec = std::env::var("ComSpec");
            Command::new(cmd_spec.unwrap_or_else(|_| "cmd.exe".to_owned()))
        } else {
            Command::new("/bin/sh")
        };
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let quoted = if need_cmd_quote(&command) {
                format!("\"{}\"", command)
            } else {
                command
            };
            cmd.arg("/c").raw_arg(quoted);
        }
        #[cfg(not(windows))]
        {
            cmd.arg("-c").arg(command);
        }
        cmd
    } else {
        Command::new(command)
    }
}

#[cfg(any(windows, test))]
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
