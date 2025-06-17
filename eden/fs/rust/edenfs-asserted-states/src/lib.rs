/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::path::PathBuf;

use edenfs_error::Result;

#[allow(dead_code)]
pub struct StreamingChangesClient {
    mount_point: PathBuf,
}

impl StreamingChangesClient {
    pub fn new(mount_point: PathBuf) -> Self {
        StreamingChangesClient { mount_point }
    }

    pub fn state_enter(&self, _state: &str) -> Result<()> {
        Ok(())
    }

    pub fn state_leave(&self, _state: &str) -> Result<()> {
        Ok(())
    }

    pub fn get_asserted_states(&self) -> Result<HashSet<String>> {
        Ok(HashSet::new())
    }

    pub fn is_state_asserted(&self, _state: &str) -> Result<bool> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use fbinit::FacebookInit;

    use crate::*;

    #[fbinit::test]
    fn test_get_asserted_states_empty(_fb: FacebookInit) -> anyhow::Result<()> {
        let mount_point = std::env::temp_dir().join("test_mount");
        let client = StreamingChangesClient::new(mount_point);
        let asserted_states = client.get_asserted_states()?;
        assert!(asserted_states.is_empty());
        Ok(())
    }
}
