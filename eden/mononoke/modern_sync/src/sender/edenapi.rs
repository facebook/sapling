/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Result;
use async_trait::async_trait;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use context::CoreContext;
use edenapi::Client;
use edenapi::HttpClientBuilder;
use edenapi::HttpClientConfig;
use edenapi::SaplingRemoteApi;
use edenapi_types::AnyFileContentId;
use edenapi_types::HgFilenodeData;
use edenapi_types::UploadTreeEntry;
use futures::TryStreamExt;
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

    async fn upload_tree(&self, trees: Vec<UploadTreeEntry>) -> Result<()> {
        let res = self.client.upload_trees_batch(trees).await?;
        info!(
            &self.logger,
            "Upload tree response: {:?}",
            res.entries.try_collect::<Vec<_>>().await?
        );
        Ok(())
    }

    async fn upload_filenodes(&self, filenodes: Vec<HgFilenodeData>) -> Result<()> {
        let res = self.client.upload_filenodes_batch(filenodes).await?;
        info!(
            &self.logger,
            "Upload filenodes response: {:?}",
            res.entries.try_collect::<Vec<_>>().await?
        );
        Ok(())
    }
}
