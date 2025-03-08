/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Error;
use anyhow::Result;
use bytes::BytesMut;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use cloned::cloned;
use context::CoreContext;
use edenapi::api::UploadLookupPolicy;
use edenapi::paths;
use edenapi::Client;
use edenapi::HttpClientBuilder;
use edenapi::HttpClientConfig;
use edenapi::SaplingRemoteApi;
use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::LookupResponse;
use edenapi_types::LookupResult;
use edenapi_types::UploadToken;
use edenapi_types::UploadTokenData;
use futures::future::BoxFuture;
use futures::stream;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use http_client::HttpVersion;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mononoke_app::args::TLSArgs;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use repo_blobstore::RepoBlobstore;
use slog::info;
use slog::warn;
use slog::Logger;
use url::Url;
mod util;

use crate::scuba;

const MAX_RETRIES: usize = 3;

pub struct EdenapiSender {
    client: Client,
    logger: Logger,
    ctx: CoreContext,
    repo_blobstore: RepoBlobstore,
}

impl EdenapiSender {
    pub async fn new(
        url: Url,
        reponame: String,
        logger: Logger,
        tls_args: TLSArgs,
        ctx: CoreContext,
        repo_blobstore: RepoBlobstore,
    ) -> Result<Self> {
        let ci = ClientInfo::new_with_entry_point(ClientEntryPoint::ModernSync)?.to_json()?;
        let http_config = HttpClientConfig {
            cert_path: Some(tls_args.tls_certificate.into()),
            key_path: Some(tls_args.tls_private_key.into()),
            ca_path: Some(tls_args.tls_ca.into()),
            client_info: Some(ci),
            ..Default::default()
        };

        info!(logger, "Connecting to {}", url.to_string());

        let client = HttpClientBuilder::new()
            .repo_name(&reponame)
            .server_url(url)
            .http_config(http_config)
            .http_version(HttpVersion::V11)
            .timeout(Duration::from_secs(300))
            .build()?;

        client.health().await?;

        Ok(Self {
            client,
            logger,
            ctx,
            repo_blobstore,
        })
    }

    pub async fn upload_contents(&self, contents: Vec<ContentId>) -> Result<()> {
        self.with_retry(|this| this.upload_contents_attempt(contents.clone()).boxed())
            .await
    }

    async fn upload_contents_attempt(&self, contents: Vec<ContentId>) -> Result<()> {
        let repo_blobstore = self.repo_blobstore.clone();
        let ctx = self.ctx.clone();
        let len = contents.len();
        let full_items = stream::iter(contents)
            .map(|id| {
                cloned!(ctx, repo_blobstore);
                async move {
                    let bytes = filestore::fetch(repo_blobstore, ctx, &id.into())
                        .await?
                        .ok_or(anyhow!("Content is not found (which should never happen"))?
                        .try_collect::<BytesMut>()
                        .await?;
                    Ok::<_, Error>((
                        AnyFileContentId::ContentId(id.into()),
                        bytes.freeze().into(),
                    ))
                }
            })
            .buffer_unordered(len)
            .try_collect::<Vec<(AnyFileContentId, minibytes::Bytes)>>()
            .await?;

        let expected_responses = full_items.len();
        let response = self
            .client
            .process_files_upload(full_items, None, None, UploadLookupPolicy::SkipLookup)
            .await?;

        let ids = response
            .entries
            .try_collect::<Vec<_>>()
            .await?
            .iter()
            .map(|e| e.data.id)
            .collect::<Vec<_>>();

        scuba::log_edenapi_stats(
            ctx.scuba().clone(),
            &response.stats.await?,
            paths::UPLOAD_FILE,
            ids.clone(),
        );

        ensure!(
            expected_responses == ids.len(),
            "Content upload: Expected {} responses, got {}",
            expected_responses,
            ids.len()
        );

        Ok(())
    }

    pub async fn upload_trees(&self, trees: Vec<HgManifestId>) -> Result<()> {
        self.with_retry(|this| this.upload_trees_attempt(trees.clone()).boxed())
            .await
    }

    async fn upload_trees_attempt(&self, trees: Vec<HgManifestId>) -> Result<()> {
        let batch_len = trees.len();
        let entries = stream::iter(trees.clone())
            .map(|mf_id| {
                let ctx = self.ctx.clone();
                let repo_blobstore = self.repo_blobstore.clone();
                async move { util::from_tree_to_entry(mf_id, &ctx, &repo_blobstore).await }
            })
            // Modern sync controls the size of the dequeue batch, so we
            // can read all the tree blobs concurrently.
            .buffer_unordered(batch_len)
            .try_collect::<Vec<_>>()
            .await?;

        let expected_responses = entries.len();
        let res = self.client.upload_trees_batch(entries).await?;
        let ids = res
            .entries
            .try_collect::<Vec<_>>()
            .await?
            .iter()
            .map(|e| e.token.data.id)
            .collect::<Vec<_>>();

        scuba::log_edenapi_stats(
            self.ctx.scuba().clone(),
            &res.stats.await?,
            paths::UPLOAD_TREES,
            ids.clone(),
        );

        ensure!(
            expected_responses == ids.len(),
            "Trees upload: Expected {} responses, got {}",
            expected_responses,
            ids.len(),
        );
        Ok(())
    }
    pub async fn upload_filenodes(&self, fn_ids: Vec<HgFileNodeId>) -> Result<()> {
        self.with_retry(|this| this.upload_filenodes_attempt(fn_ids.clone()).boxed())
            .await
    }

    async fn upload_filenodes_attempt(&self, fn_ids: Vec<HgFileNodeId>) -> Result<()> {
        let batch_len = fn_ids.len();
        let filenodes = stream::iter(fn_ids)
            .map(|file_id| {
                let ctx = self.ctx.clone();
                let repo_blobstore = self.repo_blobstore.clone();
                async move { util::from_id_to_filenode(file_id, &ctx, &repo_blobstore).await }
            })
            // Modern sync controls the size of the dequeue batch,
            // so we can read all the filenode blobs concurrently.
            .buffer_unordered(batch_len)
            .try_collect::<Vec<_>>()
            .await?;

        let expected_responses = filenodes.len();
        let res = self.client.upload_filenodes_batch(filenodes).await?;
        let ids = res
            .entries
            .try_collect::<Vec<_>>()
            .await?
            .iter()
            .map(|e| e.token.data.id)
            .collect::<Vec<_>>();

        scuba::log_edenapi_stats(
            self.ctx.scuba().clone(),
            &res.stats.await?,
            paths::UPLOAD_FILENODES,
            ids.clone(),
        );

        ensure!(
            expected_responses == ids.len(),
            "Filenodes upload: Expected {} responses, got {}",
            expected_responses,
            ids.len()
        );
        Ok(())
    }

    pub async fn set_bookmark(
        &self,
        bookmark: String,
        from: Option<HgChangesetId>,
        to: Option<HgChangesetId>,
    ) -> Result<()> {
        let res = self
            .client
            .set_bookmark(
                bookmark,
                to.map(|cs| cs.into()),
                from.map(|cs| cs.into()),
                HashMap::from([
                    ("BYPASS_READONLY".to_owned(), "true".to_owned()),
                    ("MIRROR_UPLOAD".to_owned(), "true".to_owned()),
                ]),
            )
            .await?;
        info!(&self.logger, "Moved bookmark with result {:?}", res);
        Ok(())
    }

    pub async fn upload_identical_changeset(
        &self,
        css: Vec<(HgBlobChangeset, BonsaiChangeset)>,
    ) -> Result<()> {
        self.with_retry(|this| this.upload_identical_changeset_attempt(css.clone()).boxed())
            .await
    }

    async fn upload_identical_changeset_attempt(
        &self,
        css: Vec<(HgBlobChangeset, BonsaiChangeset)>,
    ) -> Result<()> {
        let entries = stream::iter(css)
            .map(util::to_identical_changeset)
            .try_collect::<Vec<_>>()
            .await?;

        let expected_responses = entries.len();
        let res = self.client.upload_identical_changesets(entries).await?;

        let responses = res.entries.try_collect::<Vec<_>>().await?;
        ensure!(
            expected_responses == responses.len(),
            "Not all changesets were uploaded"
        );
        let ids = responses
            .iter()
            .map(|r| r.token.data.id)
            .collect::<Vec<_>>();

        scuba::log_edenapi_stats(
            self.ctx.scuba().clone(),
            &res.stats.await?,
            paths::UPLOAD_IDENTICAL_CHANGESET,
            ids.clone(),
        );

        info!(&self.logger, "Uploaded changesets: {:?}", ids);

        Ok(())
    }

    pub async fn filter_existing_commits(
        &self,
        ids: Vec<(HgChangesetId, ChangesetId)>,
    ) -> Result<Vec<ChangesetId>> {
        let hgids = ids
            .clone()
            .iter()
            .map(|(hgid, _)| AnyId::HgChangesetId(hgid.clone().into()))
            .collect::<Vec<_>>();
        let res = self.client.lookup_batch(hgids, None, None).await?;
        let missing = get_missing_in_order(res, ids);
        Ok(missing)
    }

    async fn with_retry<'t, T>(
        &'t self,
        func: impl Fn(&'t Self) -> BoxFuture<'t, Result<T>>,
    ) -> Result<T> {
        let retry_count = MAX_RETRIES;
        with_retry(retry_count, &self.logger, || func(self)).await
    }
}

async fn with_retry<'t, T>(
    max_retry_count: usize,
    logger: &Logger,
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
                warn!(
                    logger,
                    "Found error: {:?}, retrying attempt #{}", e, attempt
                );
                tokio::time::sleep(Duration::from_secs(attempt as u64 + 1)).await;
            }
        }
        attempt += 1;
    }
}

fn get_missing_in_order(
    lookup_res: Vec<LookupResponse>,
    ids: Vec<(HgChangesetId, ChangesetId)>,
) -> Vec<ChangesetId> {
    let present_ids: HashSet<_> = lookup_res
        .into_iter()
        .filter_map(|r| match r.result {
            LookupResult::Present(UploadToken {
                data:
                    UploadTokenData {
                        id,
                        bubble_id: _,
                        metadata: _,
                    },
                signature: _,
            }) => Some(id),
            _ => None,
        })
        .collect();

    let missing: Vec<_> = ids
        .into_iter()
        .filter(|(hgid, _)| !present_ids.contains(&AnyId::HgChangesetId((*hgid).into())))
        .map(|(_, csid)| csid)
        .collect();
    missing
}

#[cfg(test)]
mod test {

    use edenapi_types::IndexableId;
    use edenapi_types::LookupResponse;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_mpath_element_size() {
        let cs_id1 = ChangesetId::from_bytes([0; 32]).unwrap();
        let hg_id1 = HgChangesetId::from_bytes(&[1; 20]).unwrap();

        let cs_id2 = ChangesetId::from_bytes([1; 32]).unwrap();
        let hg_id2 = HgChangesetId::from_bytes(&[2; 20]).unwrap();

        let response1 = LookupResponse {
            result: LookupResult::NotPresent(IndexableId {
                id: AnyId::BonsaiChangesetId(cs_id1.into()),
                bubble_id: None,
            }),
        };

        let response2 = LookupResponse {
            result: LookupResult::NotPresent(IndexableId {
                id: AnyId::BonsaiChangesetId(cs_id2.into()),
                bubble_id: None,
            }),
        };

        // Simulate responses in inverted order
        let responses = vec![response2, response1];

        // This should preserve the ids order
        let missing = get_missing_in_order(responses, vec![(hg_id1, cs_id1), (hg_id2, cs_id2)]);
        assert_eq!(missing, vec![cs_id1, cs_id2]);
    }
}
