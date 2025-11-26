/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use edenapi_types::AnyFileContentId;
use futures::FutureExt;
use futures::future::BoxFuture;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::blobs::HgBlobChangeset;
use minibytes::Bytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;

use crate::sender::edenapi::EdenapiSender;

const MAX_RETRIES: usize = 3;

pub struct RetryEdenapiSender {
    inner: Arc<dyn EdenapiSender + Send + Sync>,
}

impl RetryEdenapiSender {
    pub fn new(inner: Arc<dyn EdenapiSender + Send + Sync>) -> Self {
        Self { inner }
    }

    async fn with_retry<'t, T>(
        &'t self,
        func: impl Fn(&'t Self) -> BoxFuture<'t, Result<T>>,
    ) -> Result<T> {
        let retry_count = MAX_RETRIES;
        with_retry(retry_count, || func(self)).await
    }
}

#[async_trait]
impl EdenapiSender for RetryEdenapiSender {
    async fn upload_contents(&self, contents: Vec<(AnyFileContentId, Bytes)>) -> Result<()> {
        self.with_retry(|this| this.inner.upload_contents(contents.clone()).boxed())
            .await
    }

    async fn upload_trees(&self, trees: Vec<HgManifestId>) -> Result<()> {
        self.with_retry(|this| this.inner.upload_trees(trees.clone()).boxed())
            .await
    }

    async fn upload_filenodes(&self, fn_ids: Vec<HgFileNodeId>) -> Result<()> {
        self.with_retry(|this| this.inner.upload_filenodes(fn_ids.clone()).boxed())
            .await
    }

    async fn set_bookmark(
        &self,
        bookmark: String,
        from: Option<HgChangesetId>,
        to: Option<HgChangesetId>,
    ) -> Result<()> {
        self.inner.set_bookmark(bookmark, from, to).await
    }

    async fn upload_identical_changeset(
        &self,
        css: Vec<(HgBlobChangeset, BonsaiChangeset)>,
    ) -> Result<()> {
        self.with_retry(|this| this.inner.upload_identical_changeset(css.clone()).boxed())
            .await
    }

    async fn filter_existing_commits(
        &self,
        ids: Vec<(HgChangesetId, ChangesetId)>,
    ) -> Result<Vec<ChangesetId>> {
        self.inner.filter_existing_commits(ids).await
    }

    async fn read_bookmark(&self, bookmark: String) -> Result<Option<HgChangesetId>> {
        self.inner.read_bookmark(bookmark).await
    }
}

async fn with_retry<'t, T>(
    max_retry_count: usize,
    func: impl Fn() -> BoxFuture<'t, Result<T>>,
) -> Result<T> {
    let mut attempt = 0usize;
    loop {
        let result = func().await;
        if attempt >= max_retry_count {
            return result;
        }
        match result {
            Ok(result) => return Ok(result),
            Err(e) => {
                tracing::warn!("Found error: {:?}, retrying attempt #{}", e, attempt);
                tokio::time::sleep(Duration::from_secs(attempt as u64 + 1)).await;
            }
        }
        attempt += 1;
    }
}
