/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub use anyhow::anyhow;
pub use anyhow::Result;
pub use async_trait::async_trait;
pub use cas_client::CasClient;
pub use once_cell::sync::OnceCell;
pub use tracing;
pub use types::Blake3;
pub use types::CasDigest;
pub use types::CasDigestType;

#[macro_export]
macro_rules! re_client {
    ( $struct:tt ) => {
use re_client_lib::DownloadRequest;
use re_client_lib::TCode;
use re_client_lib::TDigest;
use re_client_lib::THashAlgo;

impl $struct {
    fn client(&self) -> Result<&REClient> {
        self.client.get_or_try_init(|| self.build())
    }
}

fn to_re_digest(d: &$crate::CasDigest) -> TDigest {
    TDigest {
        hash: d.hash.to_hex(),
        size_in_bytes: d.size as i64,
        hash_algo: Some(THashAlgo::BLAKE3),
        ..Default::default()
    }
}

fn from_re_digest(d: &TDigest) -> $crate::Result<$crate::CasDigest> {
    Ok($crate::CasDigest {
        hash: $crate::Blake3::from_hex(d.hash.as_bytes())?,
        size: d.size_in_bytes as u64,
    })
}

#[$crate::async_trait]
impl $crate::CasClient for $struct {
    async fn fetch(
        &self,
        digests: &[$crate::CasDigest],
        log_name: $crate::CasDigestType,
    ) -> $crate::Result<Vec<($crate::CasDigest, Result<Option<Vec<u8>>>)>> {

        $crate::tracing::debug!(target: "cas", concat!(stringify!($struct), " fetching {} {}(s)"), digests.len(), log_name);

        let request = DownloadRequest {
            inlined_digests: Some(digests.iter().map(to_re_digest).collect()),
            throw_on_error: false,
            ..Default::default()
        };

        self.client()?
            .download(self.metadata.clone(), request)
            .await?
            .inlined_blobs
            .unwrap_or_default()
            .into_iter()
            .map(|blob| {
                let digest = from_re_digest(&blob.digest)?;
                match blob.status.code {
                    TCode::OK => Ok((digest, Ok(Some(blob.blob)))),
                    TCode::NOT_FOUND => Ok((digest, Ok(None))),
                    _ => Ok((
                        digest,
                        Err($crate::anyhow!(
                            "bad status (code={}, message={}, group={})",
                            blob.status.code,
                            blob.status.message,
                            blob.status.group
                        )),
                    )),
                }
            })
            .collect::<$crate::Result<Vec<_>>>()
    }
}
};
}
