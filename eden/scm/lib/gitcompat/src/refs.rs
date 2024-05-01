/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Component;
use std::path::Path;

use anyhow::Result;
use fs_err as fs;
use tracing::debug;
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
            let data = data.trim_end();
            if let Ok(id) = HgId::from_hex(data.as_bytes()) {
                debug!("HEAD is a hash: {}", data);
                return Ok(id);
            }
            if let Some(ref_name) = data.strip_prefix("ref: ") {
                // Attempt to resolve the reference directly by reading files.
                if let Some(id) = resolve_ref(git_dir, ref_name) {
                    debug!("HEAD is a ref: {} -> {}", ref_name, id);
                    return Ok(id);
                }
            }
            maybe_dancling = data.starts_with("ref: refs/heads/");
        }

        // Fallback to `git show-ref`, the authentic way to resolve the ref.
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
            debug!("HEAD resolved via show-ref: {}", id);
            return Ok(id);
        }

        let str_out = String::from_utf8_lossy(&out.stdout);
        anyhow::bail!("Cannot resolve HEAD from {:?}", str_out);
    }
}

/// Attempt to resolve ref_name (starting with ref/) to a commit hash.
/// This is a best effort and might fail silently.
fn resolve_ref(git_dir: &Path, ref_name: &str) -> Option<HgId> {
    // Read loose ref.
    let ref_path = Path::new(ref_name);
    // Reject malicious file name.
    if ref_path
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        let path = git_dir.join(ref_path);
        if let Ok(mut data) = fs::read(path) {
            if data.ends_with(&[b'\n']) {
                data.pop();
            }
            return HgId::from_hex(&data).ok();
        }
    }

    // Read packed ref.
    if let Ok(packed_refs) = fs::read_to_string(git_dir.join("packed-refs")) {
        for line in packed_refs.lines() {
            if let Some((hex, name)) = line.split_once(' ') {
                if name == ref_name {
                    return HgId::from_hex(hex.as_bytes()).ok();
                }
            }
        }
    }

    None
}
