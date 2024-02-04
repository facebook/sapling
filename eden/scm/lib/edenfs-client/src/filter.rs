/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use tracing::warn;
use types::HgId;

pub(crate) struct FilterGenerator {
    dot_hg_path: PathBuf,
}

impl FilterGenerator {
    pub fn new(dot_hg_path: PathBuf) -> Self {
        FilterGenerator { dot_hg_path }
    }

    // Takes a commit and returns the corresponding FilterID that should be passed to Eden.
    pub fn active_filter_id(&self, commit: HgId) -> Result<Option<String>, anyhow::Error> {
        // The filter file may be in 3 different states:
        //
        // 1) It may not exist, which indicates FilteredFS is not active
        // 2) It may contain nothing which indicates that FFS is in use, but no filter is active.
        // 3) It may contain the path to the active filter.
        //
        // We error out if the path exists but we can't read the file.
        let config_contents = std::fs::read_to_string(self.dot_hg_path.join("sparse"));
        let filter_path = match config_contents {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(anyhow::anyhow!(e)),
        };

        let filter_path = filter_path.trim();

        if filter_path.is_empty() {
            return Ok(None);
        }

        let filter_path = match filter_path.strip_prefix("%include ") {
            Some(p) => p,
            None => {
                warn!("Unexpected edensparse config format: {}", filter_path);
                return Ok(None);
            }
        };

        // Eden's ObjectIDs must be durable (once they exist, Eden must always be able to derive
        // the underlying object from them). FilteredObjectIDs contain FilterIDs, and therefore we
        // must be able to re-derive the filter contents from any FilterID so that we can properly
        // reconstruct the original filtered object at any future point in time. To do this, we
        // attach a commit ID to each FilterID which allows us to read Filter file contents from
        // the repo and reconstruct any filtered object.
        //
        // We construct a FilterID in the form {filter_file_path}:{hex_commit_hash}. We need to
        // parse this later to separate the path and commit hash, so this format assumes that
        // neither the filter file or the commit hash will have ":" in them. The second restriction
        // is guaranteed (hex), the first one will need to be enforced by us.
        Ok(Some(format!("{}:{}", filter_path, commit.to_hex())))
    }
}
