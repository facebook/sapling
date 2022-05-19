/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::BufReader;
use std::io::BufWriter;
use std::path::PathBuf;

use anyhow::Result;
use types::RepoPath;
use vfs::UpdateFlag;
use vfs::VFS;
use watchman_client::prelude::*;

use crate::filesystem::PendingChangeResult;

use super::state::StatusQuery;
use super::state::WatchmanState;

pub struct Watchman {
    vfs: VFS,
}

impl Watchman {
    pub fn new(root: PathBuf) -> Result<Self> {
        Ok(Watchman {
            vfs: VFS::new(root)?,
        })
    }

    pub async fn pending_changes(
        &self,
    ) -> Result<impl Iterator<Item = Result<PendingChangeResult>>> {
        let state_file = RepoPath::from_str("fsmonitor.state")?;

        let client = Connector::new().connect().await?;
        let resolved = client
            .resolve_root(CanonicalPath::canonicalize(self.vfs.root())?)
            .await?;

        let input = self.vfs.read(state_file)?.into_vec();
        let reader = BufReader::new(&*input);
        let mut state = WatchmanState::new(reader);

        let result = client
            .query::<StatusQuery>(
                &resolved,
                QueryRequestCommon {
                    since: state.get_clock(),
                    ..Default::default()
                },
            )
            .await?;
        state.merge(result);

        let mut output = vec![];
        let writer = BufWriter::new(&mut output);
        state.persist(writer);
        self.vfs.write(state_file, &output, UpdateFlag::Regular)?;

        Ok(state.pending_changes())
    }
}
