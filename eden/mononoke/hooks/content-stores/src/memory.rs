/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use context::CoreContext;
use mercurial_types::{blobs::HgBlobChangeset, FileBytes, HgChangesetId, HgFileNodeId, MPath};
use mononoke_types::FileType;
use std::collections::HashMap;

use crate::{ChangedFileType, ChangesetStore, ErrorKind, FileContentStore};

pub struct InMemoryChangesetStore {
    map_files:
        HashMap<HgChangesetId, Vec<(String, ChangedFileType, Option<(HgFileNodeId, FileType)>)>>,
    map_cs: HashMap<HgChangesetId, HgBlobChangeset>,
}

#[async_trait]
impl ChangesetStore for InMemoryChangesetStore {
    async fn get_changeset_by_changesetid<'a, 'b: 'a>(
        &'a self,
        _ctx: &'b CoreContext,
        changesetid: HgChangesetId,
    ) -> Result<HgBlobChangeset, Error> {
        match self.map_cs.get(&changesetid) {
            Some(cs) => Ok(cs.clone()),
            None => Err(ErrorKind::NoSuchChangeset(changesetid.to_string()).into()),
        }
    }

    async fn get_changed_files<'a, 'b: 'a>(
        &'a self,
        _ctx: &'b CoreContext,
        changesetid: HgChangesetId,
    ) -> Result<Vec<(String, ChangedFileType, Option<(HgFileNodeId, FileType)>)>, Error> {
        match self.map_files.get(&changesetid) {
            Some(files) => Ok(files.clone()),
            None => Err(ErrorKind::NoSuchChangeset(changesetid.to_string()).into()),
        }
    }
}

impl InMemoryChangesetStore {
    pub fn new() -> InMemoryChangesetStore {
        InMemoryChangesetStore {
            map_cs: HashMap::new(),
            map_files: HashMap::new(),
        }
    }

    pub fn insert_files(
        &mut self,
        changeset_id: HgChangesetId,
        files: Vec<(String, ChangedFileType, Option<(HgFileNodeId, FileType)>)>,
    ) {
        self.map_files.insert(changeset_id.clone(), files);
    }

    pub fn insert_changeset(&mut self, changeset_id: HgChangesetId, cs: HgBlobChangeset) {
        self.map_cs.insert(changeset_id.clone(), cs);
    }
}

#[derive(Clone)]
pub enum InMemoryFileText {
    Present(FileBytes),
    Elided(u64),
}

impl Into<InMemoryFileText> for Bytes {
    fn into(self) -> InMemoryFileText {
        InMemoryFileText::Present(FileBytes(self))
    }
}

impl Into<InMemoryFileText> for &str {
    fn into(self) -> InMemoryFileText {
        let bytes: Bytes = Bytes::copy_from_slice(self.as_bytes());
        bytes.into()
    }
}

impl Into<InMemoryFileText> for u64 {
    fn into(self) -> InMemoryFileText {
        InMemoryFileText::Elided(self)
    }
}

#[derive(Clone)]
pub struct InMemoryFileContentStore {
    id_to_text: HashMap<HgFileNodeId, InMemoryFileText>,
    path_to_filenode: HashMap<(HgChangesetId, MPath), HgFileNodeId>,
}

#[async_trait]
impl FileContentStore for InMemoryFileContentStore {
    async fn resolve_path<'a, 'b: 'a>(
        &'a self,
        _ctx: &'b CoreContext,
        cs_id: HgChangesetId,
        path: MPath,
    ) -> Result<Option<HgFileNodeId>, Error> {
        Ok(self.path_to_filenode.get(&(cs_id, path)).cloned())
    }

    async fn get_file_text<'a, 'b: 'a>(
        &'a self,
        _ctx: &'b CoreContext,
        id: HgFileNodeId,
    ) -> Result<Option<FileBytes>, Error> {
        self.id_to_text
            .get(&id)
            .ok_or(Error::msg("file not found"))
            .map(|c| match c {
                InMemoryFileText::Present(ref bytes) => Some(bytes.clone()),
                InMemoryFileText::Elided(_) => None,
            })
    }

    async fn get_file_size<'a, 'b: 'a>(
        &'a self,
        _ctx: &'b CoreContext,
        id: HgFileNodeId,
    ) -> Result<u64, Error> {
        self.id_to_text
            .get(&id)
            .ok_or(Error::msg("file not found"))
            .map(|c| match c {
                InMemoryFileText::Present(ref bytes) => bytes.size() as u64,
                InMemoryFileText::Elided(size) => *size,
            })
    }
}

impl InMemoryFileContentStore {
    pub fn new() -> InMemoryFileContentStore {
        InMemoryFileContentStore {
            id_to_text: HashMap::new(),
            path_to_filenode: HashMap::new(),
        }
    }

    pub fn insert(
        &mut self,
        cs_id: HgChangesetId,
        path: MPath,
        key: HgFileNodeId,
        text: impl Into<InMemoryFileText>,
    ) {
        self.id_to_text.insert(key, text.into());
        self.path_to_filenode.insert((cs_id, path), key);
    }
}
