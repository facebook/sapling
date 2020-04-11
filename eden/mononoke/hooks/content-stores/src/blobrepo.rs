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
use futures::compat::Future01CompatExt;
use mononoke_types::ContentId;

use crate::{ErrorKind, FileContentFetcher};

pub struct BlobRepoFileContentFetcher {
    pub repo: BlobRepo,
}

#[async_trait]
impl FileContentFetcher for BlobRepoFileContentFetcher {
    async fn get_file_size<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: ContentId,
    ) -> Result<u64, ErrorKind> {
        let store = self.repo.get_blobstore();
        Ok(filestore::get_metadata(&store, ctx.clone(), &id.into())
            .compat()
            .await?
            .ok_or(ErrorKind::ContentIdNotFound(id))?
            .total_size)
    }

    async fn get_file_text<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, ErrorKind> {
        let store = self.repo.get_blobstore();
        filestore::fetch_concat_opt(&store, ctx.clone(), &id.into())
            .compat()
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
