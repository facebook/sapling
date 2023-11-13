/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Implement traits from other crates.

use async_trait::async_trait;
use storemodel::FileStore;
use storemodel::KeyStore;
use storemodel::SerializationFormat;
use storemodel::TreeStore;
use types::HgId;
use types::RepoPath;

use crate::GitStore;

#[async_trait]
impl KeyStore for GitStore {
    fn get_local_content(
        &self,
        _path: &RepoPath,
        id: HgId,
    ) -> anyhow::Result<Option<minibytes::Bytes>> {
        match self.read_obj(id, git2::ObjectType::Blob) {
            Ok(data) => Ok(Some(data.into())),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn refresh(&self) -> anyhow::Result<()> {
        // We don't hold state in memory, so no need to refresh.
        Ok(())
    }
}

#[async_trait]
impl FileStore for GitStore {}

impl TreeStore for GitStore {
    fn get(&self, _path: &RepoPath, hgid: HgId) -> anyhow::Result<minibytes::Bytes> {
        let data = self.read_obj(hgid, git2::ObjectType::Tree)?;
        Ok(data.into())
    }

    fn insert(&self, _path: &RepoPath, hgid: HgId, data: minibytes::Bytes) -> anyhow::Result<()> {
        let id = self.write_obj(git2::ObjectType::Tree, data.as_ref())?;
        if id != hgid {
            anyhow::bail!("tree id mismatch: {} (written) != {} (expected)", id, hgid);
        }
        Ok(())
    }

    fn format(&self) -> SerializationFormat {
        SerializationFormat::Git
    }

    fn refresh(&self) -> anyhow::Result<()> {
        // We don't hold state in memory, so no need to refresh.
        Ok(())
    }
}
