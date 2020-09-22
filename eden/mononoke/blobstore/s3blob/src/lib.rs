/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![feature(async_closure)]

use std::time::Duration;
use std::{convert::TryInto, fmt, sync::Arc};

use anyhow::{anyhow, Context, Error, Result};
use blobstore::{Blobstore, BlobstoreGetData, BlobstoreMetadata, CountedBlobstore};
use chrono::DateTime;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future::{BoxFuture, FutureExt};
use hyper::{client::HttpConnector, StatusCode};
use hyper_tls::HttpsConnector;
use keychain_svc::{
    client::{make_KeychainService, KeychainService},
    types::GetSecretRequest,
};
use mononoke_types::BlobstoreBytes;
use native_tls::TlsConnector;
use rusoto_core::credential::StaticProvider;
use rusoto_core::{HttpClient, Region, RusotoError};
use rusoto_s3::{
    GetObjectError, GetObjectRequest, HeadObjectError, HeadObjectRequest, PutObjectRequest,
    S3Client, S3,
};
use sha1::{Digest, Sha1};
use srclient::SRChannelBuilder;
use tokio::io::AsyncReadExt;

pub const KEY_ID_NAME: &str = "S3_KEY";
pub const KEY_SECRET_NAME: &str = "S3_SECRET";

async fn get_s3_client(
    fb: FacebookInit,
    keychain_group: String,
    region_name: String,
    endpoint: String,
) -> Result<S3Client, Error> {
    let tls_connector: TlsConnector = TlsConnector::builder()
        // TODO: T75040236 change that after getting dicision from POSIX team
        // about self-signed certificates for darkisilon
        .danger_accept_invalid_certs(true)
        .build()?;
    let tls_connector = tokio_tls::TlsConnector::from(tls_connector);

    let mut http_connector = HttpConnector::new();
    http_connector.enforce_http(false);
    http_connector.set_keepalive(Some(Duration::from_secs(1)));

    let https_connector = HttpsConnector::from((http_connector, tls_connector));

    let http_client = HttpClient::from_connector(https_connector);

    let keychain_client =
        SRChannelBuilder::from_service_name(fb, &KeychainService::sr_service_name)?
            .build_client(make_KeychainService)?;

    let request = GetSecretRequest {
        name: KEY_ID_NAME.to_string(),
        author: None,
        getFromGit: None,
        group: keychain_group.clone(),
    };
    let access_key = keychain_client.getSecret(&request).await?.secret;

    let request = GetSecretRequest {
        name: KEY_SECRET_NAME.to_string(),
        author: None,
        getFromGit: None,
        group: keychain_group,
    };
    let secret_access_key = keychain_client.getSecret(&request).await?.secret;

    let cred_provider = StaticProvider::new_minimal(access_key, secret_access_key);

    let region = Region::Custom {
        name: region_name,
        endpoint,
    };

    Ok(S3Client::new_with(http_client, cred_provider, region))
}

fn get_sharded_key(key: String) -> String {
    let mut hasher = Sha1::new();
    hasher.input(&key);
    let hkey = hasher.result();
    let encoded = base64::encode_config(&hkey, base64::URL_SAFE);
    format!("{}/{}/{}", &encoded[..2], &encoded[2..4], key)
}

#[derive(Clone)]
pub struct S3Blob {
    bucket: Arc<String>,
    client: Arc<S3Client>,
}

impl fmt::Debug for S3Blob {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("S3Blob")
            .field("bucket", &self.bucket)
            .field("client", &"S3Client")
            .finish()
    }
}

impl S3Blob {
    pub async fn new<T: ToString>(
        fb: FacebookInit,
        bucket: T,
        keychain_group: T,
        region_name: T,
        endpoint: T,
    ) -> Result<CountedBlobstore<Self>, Error> {
        let blob = Self {
            bucket: Arc::new(bucket.to_string()),
            client: Arc::new(
                get_s3_client(
                    fb,
                    keychain_group.to_string(),
                    region_name.to_string(),
                    endpoint.to_string(),
                )
                .await?,
            ),
        };
        Ok(CountedBlobstore::new(
            format!("s3.{}", bucket.to_string()),
            blob,
        ))
    }
}

impl Blobstore for S3Blob {
    fn get(
        &self,
        _ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        let key = get_sharded_key(key);
        let request = GetObjectRequest {
            bucket: self.bucket.to_string(),
            key,
            ..Default::default()
        };
        let client = self.client.clone();
        async move {
            let obj = match client.get_object(request).await {
                Ok(obj) => obj,
                Err(RusotoError::Service(GetObjectError::NoSuchKey(_))) => {
                    return Ok(None);
                }
                Err(RusotoError::Unknown(http_response)) => {
                    return Err(anyhow!(format!(
                        "status: {}; Error: {}",
                        http_response.status,
                        http_response.body_as_str()
                    )))
                    .with_context(|| "while fetching blob from S3");
                }
                Err(e) => {
                    return Err(Error::from(e)).with_context(|| "while fetching blob from S3");
                }
            };
            if let Some(stream) = obj.body {
                let content_length = obj
                    .content_length
                    .ok_or_else(|| anyhow!("Object response doesn't have content length field"))?
                    .try_into()?;
                let mut body = Vec::with_capacity(content_length);
                stream.into_async_read().read_to_end(&mut body).await?;
                if body.len() != content_length {
                    return Err(anyhow!(format!(
                        "Couldn't fetch the object from S3 storage. \
                        Expected {} bytes, received {}",
                        body.len(),
                        content_length
                    )));
                }
                let ctime = obj
                    .last_modified
                    .map(|t| {
                        DateTime::parse_from_rfc2822(t.as_str())
                            .map(|time| time.timestamp())
                            .ok()
                    })
                    .flatten();
                Ok(Some(BlobstoreGetData::new(
                    BlobstoreMetadata::new(ctime),
                    BlobstoreBytes::from_bytes(body),
                )))
            } else {
                Ok(None)
            }
        }
        .boxed()
    }

    fn put(
        &self,
        _ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        let key = get_sharded_key(key);
        let content: Vec<u8> = value.as_bytes().as_ref().to_vec();
        let request = PutObjectRequest {
            bucket: self.bucket.to_string(),
            key,
            body: Some(content.into()),
            ..Default::default()
        };
        let client = self.client.clone();
        async move {
            client
                .put_object(request)
                .await
                .with_context(|| "While writing blob into S3 storage")?;
            Ok(())
        }
        .boxed()
    }

    fn is_present(
        &self,
        _ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<bool, Error>> {
        let key = get_sharded_key(key);
        let request = HeadObjectRequest {
            bucket: self.bucket.to_string(),
            key,
            ..Default::default()
        };
        let client = self.client.clone();
        async move {
            match client.head_object(request).await {
                Ok(_) => return Ok(true),
                Err(RusotoError::Service(HeadObjectError::NoSuchKey(_))) => return Ok(false),
                // Currently we are not getting HeadObjectError::NoSuchKey(_) when the key is missing
                // instead we receive HTTP response with 404 status code.
                Err(RusotoError::Unknown(http_response))
                    if StatusCode::NOT_FOUND == http_response.status =>
                {
                    return Ok(false);
                }
                Err(RusotoError::Unknown(http_response)) => {
                    return Err(anyhow!(format!(
                        "status: {}; Error: {}",
                        http_response.status,
                        http_response.body_as_str()
                    )))
                    .with_context(|| "while fetching blob from S3");
                }
                Err(e) => {
                    return Err(Error::from(e)).with_context(|| "while fetching blob from S3");
                }
            };
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sharding_key() {
        let key = "repo3502.hgmanifest.sha1.ee213cd16cf68d6abc0bc98a3469a36bf4d25553".to_string();
        let skey = get_sharded_key(key.clone());
        let parts: Vec<&str> = skey.split('/').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "1H");
        assert_eq!(parts[1], "-J");
        assert_eq!(parts[2], key.as_str());
    }
}
