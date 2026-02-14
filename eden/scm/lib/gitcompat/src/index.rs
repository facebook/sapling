/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io;
use std::process::ExitStatus;

use spawn_ext::CommandExt;
use types::HgId;

use crate::RepoGit;
use crate::rungit::GitCmd;

/// Convert `git diff-index --norenames --raw -z <tree-ish>` output
/// into `git update-index -z --index-info` input.
///
/// diff-index entry format:
///   `:<old_mode> <new_mode> <old_sha> <new_sha> <status>\0<path>\0`
///
/// See https://git-scm.com/docs/git-diff-index#_raw_output_format
///
/// `git update-index --index-info` format:
///   `<old_mode> <old_sha> <stage>\t<path>\0`
///
/// See https://git-scm.com/docs/git-update-index#_using_index_info
fn diff_index_to_index_info(raw: &[u8]) -> io::Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut pos = 0;

    let parse_err = |msg: &str| io::Error::new(io::ErrorKind::InvalidData, msg.to_owned());

    while pos < raw.len() {
        // Each entry starts with ':'
        if raw[pos] != b':' {
            return Err(parse_err("missing ':' at the beginning"));
        }
        pos += 1;

        // Header ends at the first \0: "<old_mode> <new_mode> <old_sha> <new_sha> <status>"
        let header_end = raw[pos..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| parse_err("no NUL in the entry"))?
            + pos;

        let header =
            std::str::from_utf8(&raw[pos..header_end]).map_err(|e| parse_err(&e.to_string()))?;

        // Split: old_mode, new_mode, old_sha, new_sha, status
        let mut parts = header.splitn(5, ' ');
        let old_mode = parts.next().ok_or_else(|| parse_err("missing old_mode"))?;
        parts.next(); // new_mode
        let old_sha = parts.next().ok_or_else(|| parse_err("missing old_sha"))?;
        parts.next(); // new_sha
        let status = parts.next().ok_or_else(|| parse_err("missing status"))?;

        // Copy (C) and rename (R) should be opted out by the diff-index command.
        // Copied and renamed files show up as addition (A) and deletion (D) instead.
        // Unmerged (U) status should not show up as Sapling does not expose merge conflicts to Git.
        match status {
            "M" | "A" | "D" | "T" => {}
            _ => {
                return Err(parse_err(&format!(
                    "unexpected diff-index status: {status}"
                )));
            }
        }

        // Remaining: \0<path>\0
        pos = header_end + 1;
        let path_end = raw[pos..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| parse_err("missing NUL after path"))?
            + pos;
        let path = &raw[pos..path_end];
        pos = path_end + 1;

        // <mode>SP<sha1>SP<stage>TAB<path>
        result.extend_from_slice(old_mode.as_bytes());
        result.push(b' ');
        result.extend_from_slice(old_sha.as_bytes());
        result.extend_from_slice(b" 0\t");
        result.extend_from_slice(path);
        result.push(b'\0');
    }

    Ok(result)
}

impl RepoGit {
    /// Update git index for mutated paths compared to given commit.
    /// Uses `--index-info` to avoid command-line argument length limits.
    pub fn update_diff_index(&self, treeish: HgId) -> io::Result<ExitStatus> {
        let hex = treeish.to_hex();
        let output = self.call(
            "diff-index",
            &["--cached", "--no-renames", "--raw", "-z", &hex],
        )?;

        let index_info = diff_index_to_index_info(&output.stdout)?;

        let mut cmd = self.git_cmd("update-index", &["-z", "--index-info"]);
        cmd.checked_run_with_stdin(&index_info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_index_to_index_info() {
        // Empty input
        assert_eq!(diff_index_to_index_info(b"").unwrap(), b"");

        let raw = b":100644 100644 aaaa bbbb M\0modifiedfile\0\
                     :000000 100644 0000 bbbb A\0addedfile\0\
                     :100755 000000 aaaa 0000 D\0deletedfile\0";
        let out = diff_index_to_index_info(raw).unwrap();
        assert_eq!(
            out,
            b"100644 aaaa 0\tmodifiedfile\0\
              000000 0000 0\taddedfile\0\
              100755 aaaa 0\tdeletedfile\0"
        );
    }

    #[test]
    fn test_diff_index_with_unexpected_status() {
        let raw = b":000000 000000 0000 0000 U\0unmergedfile\0";
        let err = diff_index_to_index_info(raw).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("unexpected diff-index status: U"));
    }
}
