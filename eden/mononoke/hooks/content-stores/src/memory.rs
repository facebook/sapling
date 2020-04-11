/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{ErrorKind, FileContentFetcher};

use async_trait::async_trait;
use bytes::Bytes;
use context::CoreContext;
use mononoke_types::ContentId;
use std::collections::HashMap;

#[derive(Clone)]
pub enum InMemoryFileText {
    Present(Bytes),
    Elided(u64),
}

impl Into<InMemoryFileText> for Bytes {
    fn into(self) -> InMemoryFileText {
        InMemoryFileText::Present(self)
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
pub struct InMemoryFileContentFetcher {
    id_to_text: HashMap<ContentId, InMemoryFileText>,
}

#[async_trait]
impl FileContentFetcher for InMemoryFileContentFetcher {
    async fn get_file_size<'a, 'b: 'a>(
        &'a self,
        _ctx: &'b CoreContext,
        id: ContentId,
    ) -> Result<u64, ErrorKind> {
        self.id_to_text
            .get(&id)
            .ok_or(ErrorKind::ContentIdNotFound(id))
            .map(|maybe_bytes| match maybe_bytes {
                InMemoryFileText::Present(bytes) => bytes.len() as u64,
                InMemoryFileText::Elided(size) => *size,
            })
    }

    async fn get_file_text<'a, 'b: 'a>(
        &'a self,
        _ctx: &'b CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, ErrorKind> {
        self.id_to_text
            .get(&id)
            .ok_or(ErrorKind::ContentIdNotFound(id))
            .map(|maybe_bytes| match maybe_bytes {
                InMemoryFileText::Present(bytes) => Some(bytes.clone()),
                InMemoryFileText::Elided(_) => None,
            })
    }
}

impl InMemoryFileContentFetcher {
    pub fn new() -> InMemoryFileContentFetcher {
        InMemoryFileContentFetcher {
            id_to_text: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: ContentId, text: impl Into<InMemoryFileText>) {
        self.id_to_text.insert(key, text.into());
    }
}
