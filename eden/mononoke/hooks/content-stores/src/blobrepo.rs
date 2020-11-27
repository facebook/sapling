/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use blobrepo::BlobRepo;
use bytes::Bytes;
use context::CoreContext;
use mononoke_types::ContentId;

use crate::{ErrorKind, FileContentFetcher};

pub struct BlobRepoFileContentFetcher {
    pub repo: BlobRepo,
}

#[async_trait]
impl FileContentFetcher for BlobRepoFileContentFetcher {
    async fn get_file_size<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<u64, ErrorKind> {
        let store = self.repo.blobstore();
        Ok(filestore::get_metadata(store, ctx, &id.into())
            .await?
            .ok_or(ErrorKind::ContentIdNotFound(id))?
            .total_size)
    }

    async fn get_file_text<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, ErrorKind> {
        let store = self.repo.blobstore();
        filestore::fetch_concat_opt(store, ctx, &id.into())
            .await?
            .ok_or(ErrorKind::ContentIdNotFound(id))
            .map(Option::Some)
    }
}

impl BlobRepoFileContentFetcher {
    pub fn new(repo: BlobRepo) -> BlobRepoFileContentFetcher {
        BlobRepoFileContentFetcher { repo }
    }
}
