/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use fs_err as fs;
use types::HgId;

use crate::rungit::RunGitOptions;

impl RunGitOptions {
    /// Resolve the hash of Git "HEAD", aka, ".".
    pub fn resolve_head(&self) -> Result<HgId> {
        // Whether HEAD might be a dancling pointer, like "ref: refs/heads/...".
        // Used to appromiately detect the "null" case.
        let mut maybe_dancling = false;
        // Attempt to look at ".git/HEAD" directly for performance.
        if let Some(git_dir) = self.git_dir.as_ref() {
            let data = fs::read_to_string(git_dir.join("HEAD"))?;
            if let Ok(id) = HgId::from_hex(data.trim_end().as_bytes()) {
                return Ok(id);
            }
            maybe_dancling = data.starts_with("ref: refs/heads/");
        }

        // Fallback to `git show-ref`
        let out = match self.call("show-ref", &["--head", "--hash", "HEAD"]) {
            Ok(out) => out,
            Err(e) => {
                // If the reference does not exist (newly created empty repo, or repo with commits
                // but without a working copy), the command will fail. Report as "null".
                if maybe_dancling {
                    return Ok(*HgId::null_id());
                }
                return Err(e.into());
            }
        };
        if let Some(data) = out.stdout.get(..HgId::hex_len()) {
            let id = HgId::from_hex(data)?;
            return Ok(id);
        }

        let str_out = String::from_utf8_lossy(&out.stdout);
        anyhow::bail!("Cannot resolve HEAD from {:?}", str_out);
    }
}
