/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use edenapi::Client;
use edenapi::HttpClientBuilder;
use edenapi::SaplingRemoteApi;
use slog::info;
use slog::Logger;
use url::Url;

use crate::sender::ModernSyncSender;

#[allow(dead_code)]
pub struct EdenapiSender {
    client: Client,
    logger: Logger,
}

impl EdenapiSender {
    pub async fn new(url: Url, reponame: String, logger: Logger) -> Result<Self> {
        info!(logger, "Connectign to {}", url.to_string());

        let client = HttpClientBuilder::new()
            .repo_name(&reponame)
            .server_url(url)
            .build()?;

        let res = client.health().await;
        info!(logger, "Health check outcome: {:?}", res);
        Ok(Self { client, logger })
    }
}
impl ModernSyncSender for EdenapiSender {
    fn upload_content(
        &self,
        content_id: mononoke_types::ContentId,
        _blob: mononoke_types::FileContents,
    ) {
        info!(&self.logger, "Uploading content with id: {:?}", content_id)
    }
}
