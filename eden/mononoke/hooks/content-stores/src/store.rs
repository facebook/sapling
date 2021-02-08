/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ErrorKind;

use async_trait::async_trait;
use bookmarks::BookmarkName;
use bytes::Bytes;
use context::CoreContext;
use mononoke_types::{ContentId, MPath};
use std::collections::HashMap;

#[async_trait]
pub trait FileContentFetcher: Send + Sync {
    async fn get_file_size<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<u64, ErrorKind>;

    async fn get_file_text<'a>(
        &'a self,
        ctx: &'a CoreContext,
        id: ContentId,
    ) -> Result<Option<Bytes>, ErrorKind>;

    async fn find_content<'a>(
        &'a self,
        ctx: &'a CoreContext,
        bookmark: BookmarkName,
        paths: Vec<MPath>,
    ) -> Result<HashMap<MPath, PathContent>, ErrorKind>;
}

#[derive(Clone)]
pub enum PathContent {
    Directory,
    File(ContentId),
}
