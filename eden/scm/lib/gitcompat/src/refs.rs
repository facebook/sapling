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
        // Attempt to look at ".git/HEAD" directly for performance.
        if let Some(git_dir) = self.git_dir.as_ref() {
            let data = fs::read_to_string(git_dir.join("HEAD"))?;
            let data = data.trim_end();
            if let Ok(id) = HgId::from_hex(data.as_bytes()) {
                return Ok(id);
            }
        }

        // Fallback to `git show-ref`
        let out = self.call("show-ref", &["--head", "--hash", "HEAD"])?;
        if let Some(data) = out.stdout.get(..HgId::hex_len()) {
            let id = HgId::from_hex(data)?;
            return Ok(id);
        }

        let str_out = String::from_utf8_lossy(&out.stdout);
        anyhow::bail!("Cannot resolve HEAD from {:?}", str_out);
    }
}
