/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Result;
use edenapi::Client;
use edenapi::HttpClientBuilder;
use edenapi::HttpClientConfig;
use edenapi::SaplingRemoteApi;
use mononoke_app::args::TLSArgs;
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
    pub async fn new(
        url: Url,
        reponame: String,
        logger: Logger,
        tls_args: TLSArgs,
    ) -> Result<Self> {
        let http_config = HttpClientConfig {
            cert_path: Some(tls_args.tls_certificate.into()),
            key_path: Some(tls_args.tls_private_key.into()),
            ca_path: Some(tls_args.tls_ca.into()),
            convert_cert: false,

            client_info: None,
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
