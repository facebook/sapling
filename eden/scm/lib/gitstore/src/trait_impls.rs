/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Implement traits from other crates.

use async_trait::async_trait;
use storemodel::FileStore;
use storemodel::InsertOpts;
use storemodel::KeyStore;
use storemodel::Kind;
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
        match self.read_obj(id, git2::ObjectType::Any) {
            Ok(data) => Ok(Some(data.into())),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn insert_data(&self, opts: InsertOpts, path: &RepoPath, data: &[u8]) -> anyhow::Result<HgId> {
        let kind = match opts.kind {
            Kind::File => git2::ObjectType::Blob,
            Kind::Tree => git2::ObjectType::Tree,
        };
        let id = self.write_obj(kind, data)?;
        if let Some(forced_id) = opts.forced_id {
            if forced_id.as_ref() != &id {
                anyhow::bail!("hash mismatch when writing {}@{}", path, forced_id);
            }
        }
        Ok(id)
    }

    fn format(&self) -> SerializationFormat {
        SerializationFormat::Git
    }

    fn refresh(&self) -> anyhow::Result<()> {
        // We don't hold state in memory, so no need to refresh.
        Ok(())
    }
}

#[async_trait]
impl FileStore for GitStore {}

impl TreeStore for GitStore {}
