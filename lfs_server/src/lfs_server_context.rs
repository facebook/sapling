// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::fmt::{Arguments, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use failure::Error;
use futures_util::{
    compat::{Future01CompatExt, Stream01CompatExt},
    TryStreamExt,
};
use gotham::state::{FromState, State};
use gotham_derive::StateData;
use http::uri::{Authority, Parts, PathAndQuery, Scheme, Uri};
use hyper::{Body, Request};
use slog::Logger;

use blobrepo::BlobRepo;
use context::CoreContext;
use failure_ext::chain::ChainExt;
use hyper::{client::HttpConnector, Client};
use hyper_openssl::HttpsConnector;
use mononoke_types::ContentId;

use crate::errors::ErrorKind;
use crate::protocol::{RequestBatch, RequestObject, ResponseBatch};

pub type HttpsHyperClient = Client<HttpsConnector<HttpConnector>>;

struct LfsServerContextInner {
    logger: Logger,
    repositories: HashMap<String, BlobRepo>,
    client: Arc<HttpsHyperClient>,
    server: Arc<ServerUris>,
}

#[derive(Clone, StateData)]
pub struct LfsServerContext {
    inner: Arc<Mutex<LfsServerContextInner>>,
}

#[derive(Clone, StateData)]
pub struct LoggingContext {
    pub repository: String,
    pub error_msg: Option<String>,
    pub response_size: Option<u64>,
    pub duration: Option<Duration>,
}

impl LoggingContext {
    pub fn new(repository: String) -> Self {
        Self {
            repository,
            error_msg: None,
            response_size: None,
            duration: None,
        }
    }

    pub fn set_error_msg(&mut self, error_msg: String) {
        self.error_msg = Some(error_msg);
    }

    pub fn set_response_size(&mut self, size: u64) {
        self.response_size = Some(size);
    }

    pub fn set_duration(&mut self, duration: Duration) {
        self.duration = Some(duration);
    }
}

impl LfsServerContext {
    pub fn new(
        logger: Logger,
        repositories: HashMap<String, BlobRepo>,
        server: ServerUris,
    ) -> Result<Self, Error> {
        // TODO: Configure threads?
        let connector = HttpsConnector::new(4)
            .map_err(Error::from)
            .chain_err(ErrorKind::HttpClientInitializationFailed)?;
        let client = Client::builder().build(connector);

        let inner = LfsServerContextInner {
            logger,
            repositories,
            server: Arc::new(server),
            client: Arc::new(client),
        };

        Ok(LfsServerContext {
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    pub fn request(&self, repository: String) -> Result<RequestContext, Error> {
        let inner = self.inner.lock().expect("poisoned lock");

        match inner.repositories.get(&repository) {
            Some(repo) => Ok(RequestContext {
                ctx: CoreContext::new_with_logger(inner.logger.clone()),
                repo: repo.clone(),
                uri_builder: UriBuilder {
                    repository,
                    server: inner.server.clone(),
                },
                client: inner.client.clone(),
            }),
            None => Err(ErrorKind::RepositoryDoesNotExist(repository).into()),
        }
    }
}

#[derive(Clone)]
pub struct RequestContext {
    pub ctx: CoreContext,
    pub repo: BlobRepo,
    pub uri_builder: UriBuilder,
    client: Arc<HttpsHyperClient>,
}

impl RequestContext {
    pub fn instantiate(state: &mut State, repository: String) -> Result<Self, Error> {
        state.put(LoggingContext::new(repository.clone()));
        let ctx = LfsServerContext::borrow_from(&state);
        ctx.request(repository)
    }

    pub async fn dispatch(&self, request: Request<Body>) -> Result<Body, Error> {
        let res = self
            .client
            .request(request)
            .compat()
            .await
            .chain_err(ErrorKind::UpstreamDidNotRespond)?;

        let (head, body) = res.into_parts();

        if !head.status.is_success() {
            let body = body.compat().try_concat().await?;

            return Err(ErrorKind::UpstreamError(
                head.status,
                String::from_utf8_lossy(&body).to_string(),
            )
            .into());
        }

        Ok(body)
    }

    pub async fn upstream_batch(
        &self,
        batch: &RequestBatch,
    ) -> Result<Option<ResponseBatch>, Error> {
        let uri = match self.uri_builder.upstream_batch_uri()? {
            Some(uri) => uri,
            None => {
                return Ok(None);
            }
        };

        let body: Bytes = serde_json::to_vec(&batch)
            .chain_err(ErrorKind::SerializationFailed)?
            .into();

        let req = Request::post(uri).body(body.into())?;

        let res = self
            .dispatch(req)
            .await?
            .compat()
            .try_concat()
            .await
            .chain_err(ErrorKind::UpstreamBatchNoResponse)?;

        let batch = serde_json::from_slice::<ResponseBatch>(&res)
            .chain_err(ErrorKind::UpstreamBatchInvalid)?;

        Ok(Some(batch))
    }
}

#[derive(Clone)]
pub struct UriBuilder {
    pub repository: String,
    pub server: Arc<ServerUris>,
}

impl UriBuilder {
    pub fn upload_uri(&self, object: &RequestObject) -> Result<Uri, Error> {
        self.server
            .self_uri
            .build(format_args!(
                "{}/upload/{}/{}",
                &self.repository, object.oid, object.size
            ))
            .chain_err(ErrorKind::UriBuilderFailed("upload_uri"))
            .map_err(Error::from)
    }

    pub fn download_uri(&self, content_id: &ContentId) -> Result<Uri, Error> {
        self.server
            .self_uri
            .build(format_args!("{}/download/{}", &self.repository, content_id))
            .chain_err(ErrorKind::UriBuilderFailed("download_uri"))
            .map_err(Error::from)
    }

    pub fn upstream_batch_uri(&self) -> Result<Option<Uri>, Error> {
        self.server
            .upstream_uri
            .as_ref()
            .map(|uri| {
                uri.build(format_args!("objects/batch"))
                    .chain_err(ErrorKind::UriBuilderFailed("upstream_batch_uri"))
                    .map_err(Error::from)
            })
            .transpose()
    }
}

fn parse_and_check_uri(src: &str) -> Result<BaseUri, Error> {
    let uri = src.parse::<Uri>().map_err(Error::from)?;

    let Parts {
        scheme,
        authority,
        path_and_query,
        ..
    } = uri.into_parts();

    Ok(BaseUri {
        scheme: scheme.ok_or_else(|| ErrorKind::InvalidUri(src.to_string(), "missing scheme"))?,
        authority: authority
            .ok_or_else(|| ErrorKind::InvalidUri(src.to_string(), "missing authority"))?,
        path_and_query,
    })
}

#[derive(Debug)]
pub struct ServerUris {
    /// The root URL to use when composing URLs for this LFS server
    self_uri: BaseUri,
    /// The URL for an upstream LFS server
    upstream_uri: Option<BaseUri>,
}

impl ServerUris {
    pub fn new(self_uri: &str, upstream_uri: Option<&str>) -> Result<Self, Error> {
        Ok(Self {
            self_uri: parse_and_check_uri(self_uri)?,
            upstream_uri: upstream_uri.map(parse_and_check_uri).transpose()?,
        })
    }
}

#[derive(Debug)]
struct BaseUri {
    scheme: Scheme,
    authority: Authority,
    path_and_query: Option<PathAndQuery>,
}

impl BaseUri {
    pub fn build(&self, args: Arguments) -> Result<Uri, Error> {
        let mut p = String::new();
        if let Some(ref path_and_query) = self.path_and_query {
            write!(&mut p, "{}", path_and_query)?;
            if !path_and_query.path().ends_with("/") {
                write!(&mut p, "{}", "/")?;
            }
        }
        p.write_fmt(args)?;

        Uri::builder()
            .scheme(self.scheme.clone())
            .authority(self.authority.clone())
            .path_and_query(&p[..])
            .build()
            .map_err(Error::from)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use mononoke_types::{hash::Sha256, ContentId};
    use std::str::FromStr;

    const ONES_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const SIZE: u64 = 123;

    fn obj() -> Result<RequestObject, Error> {
        Ok(RequestObject {
            oid: Sha256::from_str(ONES_HASH)?,
            size: SIZE,
        })
    }

    fn content_id() -> Result<ContentId, Error> {
        Ok(ContentId::from_str(ONES_HASH)?)
    }

    fn uri_builder(self_uri: &str, upstream_uri: &str) -> Result<UriBuilder, Error> {
        let server = ServerUris::new(self_uri, Some(upstream_uri))?;
        Ok(UriBuilder {
            repository: "repo123".to_string(),
            server: Arc::new(server),
        })
    }

    #[test]
    fn test_basic_upload_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com", "http://bar.com")?;
        assert_eq!(
            b.upload_uri(&obj()?)?.to_string(),
            format!("http://foo.com/repo123/upload/{}/{}", ONES_HASH, SIZE),
        );
        Ok(())
    }

    #[test]
    fn test_basic_upload_uri_slash() -> Result<(), Error> {
        let b = uri_builder("http://foo.com/", "http://bar.com")?;
        assert_eq!(
            b.upload_uri(&obj()?)?.to_string(),
            format!("http://foo.com/repo123/upload/{}/{}", ONES_HASH, SIZE),
        );
        Ok(())
    }

    #[test]
    fn test_prefix_upload_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com/bar", "http://bar.com")?;
        assert_eq!(
            b.upload_uri(&obj()?)?.to_string(),
            format!("http://foo.com/bar/repo123/upload/{}/{}", ONES_HASH, SIZE),
        );
        Ok(())
    }

    #[test]
    fn test_prefix_slash_upload_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com/bar/", "http://bar.com")?;
        assert_eq!(
            b.upload_uri(&obj()?)?.to_string(),
            format!("http://foo.com/bar/repo123/upload/{}/{}", ONES_HASH, SIZE),
        );
        Ok(())
    }

    #[test]
    fn test_basic_download_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com", "http://bar.com")?;
        assert_eq!(
            b.download_uri(&content_id()?)?.to_string(),
            format!("http://foo.com/repo123/download/{}", ONES_HASH),
        );
        Ok(())
    }

    #[test]
    fn test_basic_download_uri_slash() -> Result<(), Error> {
        let b = uri_builder("http://foo.com/", "http://bar.com")?;
        assert_eq!(
            b.download_uri(&content_id()?)?.to_string(),
            format!("http://foo.com/repo123/download/{}", ONES_HASH),
        );
        Ok(())
    }

    #[test]
    fn test_prefix_download_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com/bar", "http://bar.com")?;
        assert_eq!(
            b.download_uri(&content_id()?)?.to_string(),
            format!("http://foo.com/bar/repo123/download/{}", ONES_HASH),
        );
        Ok(())
    }

    #[test]
    fn test_prefix_slash_download_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com/bar/", "http://bar.com")?;
        assert_eq!(
            b.download_uri(&content_id()?)?.to_string(),
            format!("http://foo.com/bar/repo123/download/{}", ONES_HASH),
        );
        Ok(())
    }

    #[test]
    fn test_basic_upstream_batch_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com", "http://bar.com")?;
        assert_eq!(
            b.upstream_batch_uri()?.map(|uri| uri.to_string()),
            Some(format!("http://bar.com/objects/batch")),
        );
        Ok(())
    }

    #[test]
    fn test_basic_upstream_batch_uri_slash() -> Result<(), Error> {
        let b = uri_builder("http://foo.com/", "http://bar.com")?;
        assert_eq!(
            b.upstream_batch_uri()?.map(|uri| uri.to_string()),
            Some(format!("http://bar.com/objects/batch")),
        );
        Ok(())
    }

    #[test]
    fn test_prefix_upstream_batch_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com", "http://bar.com/foo")?;
        assert_eq!(
            b.upstream_batch_uri()?.map(|uri| uri.to_string()),
            Some(format!("http://bar.com/foo/objects/batch")),
        );
        Ok(())
    }

    #[test]
    fn test_prefix_slash_upstream_batch_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com", "http://bar.com/foo/")?;
        assert_eq!(
            b.upstream_batch_uri()?.map(|uri| uri.to_string()),
            Some(format!("http://bar.com/foo/objects/batch")),
        );
        Ok(())
    }
}
