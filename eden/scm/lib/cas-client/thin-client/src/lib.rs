/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use cas_client::CasClient;
use cas_client::CasDigest;
use configmodel::Config;
use configmodel::ConfigExt;
use re_client_lib::create_default_config;
use re_client_lib::DownloadRequest;
use re_client_lib::ExternalCASDaemonAddress;
use re_client_lib::REClient;
use re_client_lib::REClientBuilder;
use re_client_lib::RemoteExecutionMetadata;
use re_client_lib::TCode;
use re_client_lib::TDigest;
use re_client_lib::THashAlgo;
use types::Blake3;

pub struct ThinCasClient {
    client: REClient,
    metadata: RemoteExecutionMetadata,
}

pub fn construct(config: &dyn Config) -> Result<Arc<dyn CasClient>> {
    ThinCasClient::from_config(config).map(|c| Arc::new(c) as Arc<dyn CasClient>)
}

impl ThinCasClient {
    pub fn from_config(config: &dyn Config) -> Result<Self> {
        let mut re_config = create_default_config();

        re_config.client_name = Some("sapling".to_string());
        re_config.quiet_mode = !config.get_or_default("cas", "verbose")?;
        re_config.features_config_path =
            "remote_execution/features/client_source_control".to_string();

        let mut builder = REClientBuilder::new(fbinit::expect_init()).with_config(re_config);

        let connection_count: u32 = config.get_or("cas", "connection-count", || 1)?;

        if let Some(port) = config.get_opt::<i32>("cas", "port")? {
            builder =
                builder.with_cas_daemon(ExternalCASDaemonAddress::port(port), connection_count);
        } else if let Some(uds_path) = config.get_opt::<String>("cas", "uds-path")? {
            builder = builder.with_cas_daemon(
                ExternalCASDaemonAddress::uds_path(uds_path),
                connection_count,
            );
        } else {
            builder = builder.with_wdb_cas_daemon(connection_count);
        }

        let use_case: String = match config.get("cas", "use-case") {
            Some(use_case) => use_case.to_string(),
            None => format!(
                "source-control-{}",
                config.must_get::<String>("remotefilelog", "reponame")?
            ),
        };

        Ok(Self {
            client: builder.build()?,
            metadata: RemoteExecutionMetadata {
                use_case_id: use_case,
                ..Default::default()
            },
        })
    }
}

fn to_re_digest(d: &CasDigest) -> TDigest {
    TDigest {
        hash: d.hash.to_hex(),
        size_in_bytes: d.size as i64,
        hash_algo: Some(THashAlgo::BLAKE3),
        ..Default::default()
    }
}

fn from_re_digest(d: &TDigest) -> Result<CasDigest> {
    Ok(CasDigest {
        hash: Blake3::from_hex(d.hash.as_bytes())?,
        size: d.size_in_bytes as u64,
    })
}

#[async_trait::async_trait]
impl CasClient for ThinCasClient {
    async fn fetch(&self, digests: &[CasDigest]) -> Result<Vec<(CasDigest, Result<Vec<u8>>)>> {
        let request = DownloadRequest {
            inlined_digests: Some(digests.iter().map(to_re_digest).collect()),
            ..Default::default()
        };

        self.client
            .download(self.metadata.clone(), request)
            .await?
            .inlined_blobs
            .unwrap_or_default()
            .into_iter()
            .map(|blob| {
                let digest = from_re_digest(&blob.digest)?;
                if blob.status.code == TCode::OK {
                    Ok((digest, Ok(blob.blob)))
                } else {
                    Ok((
                        digest,
                        Err(anyhow!(
                            "bad status (code={}, message={}, group={})",
                            blob.status.code,
                            blob.status.message,
                            blob.status.group
                        )),
                    ))
                }
            })
            .collect::<Result<Vec<_>>>()
    }
}
