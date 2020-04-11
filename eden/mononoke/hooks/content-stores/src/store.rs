/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ErrorKind;

use async_trait::async_trait;
use bytes::Bytes;
use context::CoreContext;
use mononoke_types::ContentId;

#[async_trait]
pub trait FileContentFetcher: Send + Sync {
    async fn get_file_size<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: ContentId,
    ) -> Result<u64, ErrorKind>;

    async fn get_file_text<'a, 'b: 'a>(
        &'a self,
        ctx: &'b CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, ErrorKind>;
}
