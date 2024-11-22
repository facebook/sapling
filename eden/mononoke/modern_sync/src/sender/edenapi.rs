/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use edenapi::Client;
use edenapi::HttpClientBuilder;
use url::Url;

use crate::sender::ModernSyncSender;

#[allow(dead_code)]
pub struct EdenapiSender {
    client: Client,
}

impl EdenapiSender {
    pub fn new(url: Url, reponame: String) -> Result<Self> {
        let client = HttpClientBuilder::new()
            .repo_name(&reponame)
            .server_url(url)
            .build()?;
        Ok(Self { client })
    }
}
impl ModernSyncSender for EdenapiSender {
    fn upload_content(
        &self,
        _content_id: mononoke_types::ContentId,
        _blob: mononoke_types::FileContents,
    ) {
        eprintln!("not implemented yet")
    }
}
