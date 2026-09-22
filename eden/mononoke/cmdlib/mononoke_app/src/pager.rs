/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Pipe help output through `$PAGER`.

use std::env;
use std::io;
use std::io::IsTerminal;
use std::io::Write;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;

/// Whether the pager never ran: `sh` exits 126/127 when it cannot execute
/// the `$PAGER` command, in which case nothing was displayed.
fn pager_never_ran(status: ExitStatus) -> bool {
    matches!(status.code(), Some(126) | Some(127))
}

fn nonblank_pager(pager: &str) -> Option<&str> {
    (!pager.trim().is_empty()).then_some(pager)
}

fn pager_command() -> Option<String> {
    env::var("PAGER")
        .ok()
        .and_then(|pager| nonblank_pager(&pager).map(str::to_owned))
}

/// Write `text` to `$PAGER`, returning false when paging is unavailable.
///
/// Paging applies to interactive use only: `$PAGER` must be set and stdout
/// must be a terminal. The pager runs via `sh -c`, following the git/hg
/// convention, so quoted arguments and redirections in `$PAGER` work.
/// Callers fall back to printing directly on false.
pub(crate) fn page_text(text: &str) -> bool {
    let Some(pager) = pager_command() else {
        return false;
    };
    if !io::stdout().is_terminal() {
        return false;
    }
    let mut child = match Command::new("sh")
        .arg("-c")
        .arg(&pager)
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    if let Some(mut stdin) = child.stdin.take() {
        // The user may quit the pager before we finish writing; a failed
        // write just means the output was dismissed.
        let _ = stdin.write_all(text.as_bytes());
    }
    match child.wait() {
        Ok(status) => !pager_never_ran(status),
        // wait() only fails if the child was already collected; the pager
        // may never have displayed anything, so fall back to direct print.
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::nonblank_pager;
    use super::pager_never_ran;

    #[mononoke::test]
    fn test_nonblank_pager() {
        assert_eq!(nonblank_pager(""), None, "empty PAGER disables paging");
        assert_eq!(nonblank_pager("   "), None, "blank PAGER disables paging");
        assert_eq!(
            nonblank_pager("less"),
            Some("less"),
            "bare program is passed through"
        );
        assert_eq!(
            nonblank_pager("less -R"),
            Some("less -R"),
            "PAGER with arguments is passed through verbatim"
        );
        assert_eq!(
            nonblank_pager("less \"-R -S\" | tee /tmp/help.log"),
            Some("less \"-R -S\" | tee /tmp/help.log"),
            "quotes and shell metacharacters are left for sh -c"
        );
    }

    #[cfg(unix)]
    #[mononoke::test]
    fn test_pager_never_ran() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::ExitStatus;

        assert!(
            pager_never_ran(ExitStatus::from_raw(126 << 8)),
            "sh exit 126 (not executable) means the pager never ran"
        );
        assert!(
            pager_never_ran(ExitStatus::from_raw(127 << 8)),
            "sh exit 127 (not found) means the pager never ran"
        );
        assert!(
            !pager_never_ran(ExitStatus::from_raw(0)),
            "sh exit 0 means the pager handled the output"
        );
        assert!(
            !pager_never_ran(ExitStatus::from_raw(1 << 8)),
            "other pager failures still count as paged (no double print)"
        );
    }
}
