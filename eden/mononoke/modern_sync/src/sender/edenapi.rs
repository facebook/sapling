/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Loadable;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use context::CoreContext;
use edenapi::Client;
use edenapi::HttpClientBuilder;
use edenapi::HttpClientConfig;
use edenapi::SaplingRemoteApi;
use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::HgFilenodeData;
use edenapi_types::Parents;
use edenapi_types::UploadToken;
use edenapi_types::UploadTreeEntry;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use mercurial_types::fetch_manifest_envelope;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mononoke_app::args::TLSArgs;
use mononoke_types::FileContents;
use repo_blobstore::RepoBlobstore;
use slog::info;
use slog::Logger;
use url::Url;

use crate::sender::ModernSyncSender;

#[allow(dead_code)]
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
            convert_cert: false,

            client_info: Some(ci),
            disable_tls_verification: false,
            max_concurrent_requests: None,
            unix_socket_domains: HashSet::new(),
            unix_socket_path: None,
            verbose: false,
            verbose_stats: false,
        };

        info!(logger, "Connectign to {}", url.to_string());

        let client = HttpClientBuilder::new()
            .repo_name(&reponame)
            .server_url(url)
            .http_config(http_config)
            .build()?;

        let res = client.health().await;
        info!(logger, "Health check outcome: {:?}", res);
        Ok(Self {
            client,
            logger,
            ctx,
            repo_blobstore,
        })
    }
}

#[async_trait]
impl ModernSyncSender for EdenapiSender {
    async fn upload_content(
        &self,
        content_id: mononoke_types::ContentId,
        blob: FileContents,
    ) -> Result<()> {
        info!(&self.logger, "Uploading content with id: {:?}", content_id);

        match blob {
            FileContents::Bytes(bytes) => {
                info!(&self.logger, "Uploading bytes: {:?}", bytes);
                let response = self
                    .client
                    .process_files_upload(
                        vec![(AnyFileContentId::ContentId(content_id.into()), bytes.into())],
                        None,
                        None,
                    )
                    .await?;
                info!(
                    &self.logger,
                    "Upload response: {:?}",
                    response.entries.try_collect::<Vec<_>>().await?
                );
            }
            _ => (),
        }

        Ok(())
    }

    async fn upload_trees(&self, trees: Vec<HgManifestId>) -> Result<()> {
        let entries = stream::iter(trees)
            .map(|mf_id| {
                let ctx = self.ctx.clone();
                let repo_blobstore = self.repo_blobstore.clone();
                async move { from_tree_to_entry(mf_id, &ctx, &repo_blobstore).await }
            })
            .buffer_unordered(10)
            .try_collect::<Vec<_>>()
            .await?;

        let res = self.client.upload_trees_batch(entries).await?;
        info!(
            &self.logger,
            "Upload tree response: {:?}",
            res.entries.try_collect::<Vec<_>>().await?
        );
        Ok(())
    }

    async fn upload_filenodes(&self, fn_ids: Vec<HgFileNodeId>) -> Result<()> {
        let filenodes = stream::iter(fn_ids)
            .map(|file_id| {
                let ctx = self.ctx.clone();
                let repo_blobstore = self.repo_blobstore.clone();
                async move { from_id_to_filenode(file_id, &ctx, &repo_blobstore).await }
            })
            .buffer_unordered(10)
            .try_collect::<Vec<_>>()
            .await?;

        let res = self.client.upload_filenodes_batch(filenodes).await?;
        info!(
            &self.logger,
            "Upload filenodes response: {:?}",
            res.entries.try_collect::<Vec<_>>().await?
        );
        Ok(())
    }
}

pub async fn from_tree_to_entry(
    id: HgManifestId,
    ctx: &CoreContext,
    repo_blobstore: &RepoBlobstore,
) -> Result<UploadTreeEntry> {
    let envelope = fetch_manifest_envelope(ctx, repo_blobstore, id).await?;
    let content = envelope.contents();

    let parents = match envelope.parents() {
        (None, None) => Parents::None,
        (Some(p1), None) => Parents::One(p1.into()),
        (None, Some(p2)) => Parents::One(p2.into()),
        (Some(p1), Some(p2)) => Parents::Two(p1.into(), p2.into()),
    };

    Ok(UploadTreeEntry {
        node_id: envelope.node_id().into(),
        data: content.to_vec(),
        parents,
    })
}

pub async fn from_id_to_filenode(
    file_id: HgFileNodeId,
    ctx: &CoreContext,
    repo_blobstore: &RepoBlobstore,
) -> Result<HgFilenodeData> {
    let file_node = file_id.load(ctx, repo_blobstore).await?;

    // These tokens are mostly implemented to make sure client sends content before uplaoding filenodes
    // but they're not really verified, given we're indeed sending the content, let's use a placeholder
    let content_id = file_node.content_id();
    let token = UploadToken::new_fake_token(
        AnyId::AnyFileContentId(AnyFileContentId::ContentId(content_id.into())),
        None,
    );

    Ok(HgFilenodeData {
        node_id: file_id.into_nodehash().into(),
        parents: file_node.hg_parents().into(),
        metadata: file_node.metadata().clone().to_vec(),
        file_content_upload_token: token,
    })
}
