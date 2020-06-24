/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt::{Arguments, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use anyhow::{Context, Error};
use bytes::Bytes;
use cached_config::ConfigHandle;
use futures::stream::{Stream, TryStreamExt};
use gotham::state::{FromState, State};
use gotham_derive::StateData;
use gotham_ext::body_ext::BodyExt;
use http::{
    header::HeaderMap,
    uri::{Authority, Parts, PathAndQuery, Scheme, Uri},
};
use hyper::{Body, Request};
use permission_checker::{ArcPermissionChecker, MononokeIdentitySet};
use slog::Logger;

use blobrepo::BlobRepo;
use context::CoreContext;
use hyper::{client::HttpConnector, Client};
use hyper_openssl::HttpsConnector;
use lfs_protocol::{RequestBatch, RequestObject, ResponseBatch};
use mononoke_types::hash::Sha256;
use mononoke_types::ContentId;

use crate::config::ServerConfig;
use crate::errors::{ErrorKind, LfsServerContextErrorKind};
use crate::middleware::{ClientIdentity, LfsMethod, RequestContext};

pub type HttpsHyperClient = Client<HttpsConnector<HttpConnector>>;

// For some reason Source Control uses the read action to decide if a user can write to a repo...
const ACL_CHECK_ACTION: &str = "read";

struct LfsServerContextInner {
    repositories: HashMap<String, (BlobRepo, ArcPermissionChecker)>,
    client: Arc<HttpsHyperClient>,
    server: Arc<ServerUris>,
    always_wait_for_upstream: bool,
    max_upload_size: Option<u64>,
    config_handle: ConfigHandle<ServerConfig>,
}

#[derive(Clone, StateData)]
pub struct LfsServerContext {
    inner: Arc<Mutex<LfsServerContextInner>>,
    will_exit: Arc<AtomicBool>,
}

impl LfsServerContext {
    pub fn new(
        repositories: HashMap<String, (BlobRepo, ArcPermissionChecker)>,
        server: ServerUris,
        always_wait_for_upstream: bool,
        max_upload_size: Option<u64>,
        will_exit: Arc<AtomicBool>,
        config_handle: ConfigHandle<ServerConfig>,
    ) -> Result<Self, Error> {
        let connector = HttpsConnector::new()
            .map_err(Error::from)
            .context(ErrorKind::HttpClientInitializationFailed)?;
        let client = Client::builder().build(connector);

        let inner = LfsServerContextInner {
            repositories,
            server: Arc::new(server),
            client: Arc::new(client),
            always_wait_for_upstream,
            max_upload_size,
            config_handle,
        };

        Ok(LfsServerContext {
            inner: Arc::new(Mutex::new(inner)),
            will_exit,
        })
    }

    pub async fn request(
        &self,
        ctx: CoreContext,
        repository: String,
        identities: Option<&MononokeIdentitySet>,
    ) -> Result<RepositoryRequestContext, LfsServerContextErrorKind> {
        let (repo, aclchecker, client, server, always_wait_for_upstream, max_upload_size, config) = {
            let inner = self.inner.lock().expect("poisoned lock");

            match inner.repositories.get(&repository) {
                Some((repo, aclchecker)) => (
                    repo.clone(),
                    aclchecker.clone(),
                    inner.client.clone(),
                    inner.server.clone(),
                    inner.always_wait_for_upstream,
                    inner.max_upload_size,
                    inner.config_handle.get(),
                ),
                None => {
                    return Err(LfsServerContextErrorKind::RepositoryDoesNotExist(
                        repository,
                    ))
                }
            }
        };

        if config.acl_check() {
            acl_check(aclchecker, identities, config.enforce_acl_check()).await?;
        }

        Ok(RepositoryRequestContext {
            ctx,
            repo,
            uri_builder: UriBuilder { repository, server },
            client: HttpClient::Enabled(client),
            config,
            always_wait_for_upstream,
            max_upload_size,
        })
    }

    pub fn get_config_handle(&self) -> ConfigHandle<ServerConfig> {
        self.inner
            .lock()
            .expect("poisoned lock")
            .config_handle
            .clone()
    }

    pub fn get_config(&self) -> Arc<ServerConfig> {
        let inner = self.inner.lock().expect("poisoned lock");
        inner.config_handle.get()
    }

    pub fn will_exit(&self) -> bool {
        self.will_exit.load(Ordering::Relaxed)
    }
}

async fn acl_check(
    aclchecker: ArcPermissionChecker,
    identities: Option<&MononokeIdentitySet>,
    enforce_acl_check: bool,
) -> Result<(), LfsServerContextErrorKind> {
    if let Some(identities) = identities {
        // Always make the request for permission even if enforce_acl_check is
        // false, so that we get the even logged in our system
        let acl_check = aclchecker
            .check_set(identities, &[ACL_CHECK_ACTION])
            .await
            .map_err(LfsServerContextErrorKind::PermissionCheckFailed)?;
        if !acl_check && enforce_acl_check {
            return Err(LfsServerContextErrorKind::Forbidden.into());
        } else {
            return Ok(());
        }
    } else {
        // For now, allow clients to connect that don't provide an identity. Once we know
        // all clients have idents, we can return Forbidden.
        return Ok(());
    }
}

#[derive(Clone)]
enum HttpClient {
    Enabled(Arc<HttpsHyperClient>),
    #[cfg(test)]
    Disabled,
}

#[derive(Clone)]
pub struct RepositoryRequestContext {
    pub ctx: CoreContext,
    pub repo: BlobRepo,
    pub uri_builder: UriBuilder,
    pub config: Arc<ServerConfig>,
    always_wait_for_upstream: bool,
    max_upload_size: Option<u64>,
    client: HttpClient,
}

pub struct HttpClientResponse<S> {
    headers: HeaderMap,
    body: S,
}

impl<S: Stream<Item = Result<Bytes, Error>>> HttpClientResponse<S> {
    pub async fn concat(self) -> Result<Bytes, Error> {
        let body = self.body.try_concat_body(&self.headers)?.await?;
        Ok(body)
    }

    pub async fn discard(self) -> Result<(), Error> {
        self.concat().await?;
        Ok(())
    }

    pub fn into_inner(self) -> S {
        self.body
    }
}

impl RepositoryRequestContext {
    pub async fn instantiate(
        state: &mut State,
        repository: String,
        method: LfsMethod,
    ) -> Result<Self, LfsServerContextErrorKind> {
        let req_ctx = state.borrow_mut::<RequestContext>();
        req_ctx.set_request(repository.clone(), method);

        let ctx = req_ctx.ctx.clone();

        let identities = if let Some(client_ident) = state.try_borrow::<ClientIdentity>() {
            client_ident.identities().as_ref()
        } else {
            None
        };

        let lfs_ctx = LfsServerContext::borrow_from(&state);
        lfs_ctx.request(ctx, repository, identities).await
    }

    pub fn logger(&self) -> &Logger {
        self.ctx.logger()
    }

    pub fn always_wait_for_upstream(&self) -> bool {
        self.always_wait_for_upstream
    }

    pub fn max_upload_size(&self) -> Option<u64> {
        self.max_upload_size
    }

    pub async fn dispatch(
        &self,
        request: Request<Body>,
    ) -> Result<HttpClientResponse<impl Stream<Item = Result<Bytes, Error>>>, Error> {
        #[allow(clippy::infallible_destructuring_match)]
        let client = match self.client {
            HttpClient::Enabled(ref client) => client,
            #[cfg(test)]
            HttpClient::Disabled => panic!("HttpClient is disabled in test"),
        };

        let res = client.request(request);

        // NOTE: We spawn the request on an executor because we'd like to read the response even if
        // we drop the future returned here. The reason for that is that if we don't read a
        // response, Hyper will not reuse the conneciton for its pool (which makes sense for the
        // general case: if your server is sending you 5GB of data and you drop the future, you
        // don't want to read all that later just to reuse a connection).
        let fut = async move {
            let res = res.await.context(ErrorKind::UpstreamDidNotRespond)?;

            let (head, body) = res.into_parts();

            if !head.status.is_success() {
                let body = body.try_concat_body(&head.headers)?.await?;
                return Err(ErrorKind::UpstreamError(
                    head.status,
                    String::from_utf8_lossy(&body).to_string(),
                )
                .into());
            }

            // NOTE: This buffers the response here, since all our callsites need a concatenated
            // response. If we want to add callsites that need a streaming response, we should add
            // our own wrapper type that wraps the response and the headers.
            Ok(HttpClientResponse {
                headers: head.headers,
                body: body.map_err(Error::from),
            })
        };

        tokio::spawn(fut).await?
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
            .context(ErrorKind::SerializationFailed)?
            .into();

        let req = Request::post(uri).body(body.into())?;

        let res = self
            .dispatch(req)
            .await
            .context(ErrorKind::UpstreamBatchNoResponse)?
            .concat()
            .await?;

        let batch = serde_json::from_slice::<ResponseBatch>(&res)
            .context(ErrorKind::UpstreamBatchInvalid)?;

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
            .context(ErrorKind::UriBuilderFailed("upload_uri"))
            .map_err(Error::from)
    }

    pub fn download_uri(&self, content_id: &ContentId) -> Result<Uri, Error> {
        self.server
            .self_uri
            .build(format_args!("{}/download/{}", &self.repository, content_id))
            .context(ErrorKind::UriBuilderFailed("download_uri"))
            .map_err(Error::from)
    }

    pub fn consistent_download_uri(
        &self,
        content_id: &ContentId,
        oid: Sha256,
    ) -> Result<Uri, Error> {
        self.server
            .self_uri
            .build(format_args!(
                "{}/download/{}?routing={}",
                &self.repository, content_id, oid
            ))
            .context(ErrorKind::UriBuilderFailed("consistent_download_uri"))
            .map_err(Error::from)
    }

    pub fn upstream_batch_uri(&self) -> Result<Option<Uri>, Error> {
        self.server
            .upstream_uri
            .as_ref()
            .map(|uri| {
                uri.build(format_args!("objects/batch"))
                    .context(ErrorKind::UriBuilderFailed("upstream_batch_uri"))
                    .map_err(Error::from)
            })
            .transpose()
    }
}

fn parse_and_check_uri(src: &str) -> Result<BaseUri, Error> {
    let uri = src
        .parse::<Uri>()
        .with_context(|| ErrorKind::InvalidUri(src.to_string(), "invalid uri"))?;

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
    use blobrepo_factory::TestRepoBuilder;
    use fbinit::FacebookInit;
    use lfs_protocol::Sha256 as LfsSha256;
    use mononoke_types::{hash::Sha256, ContentId};
    use std::str::FromStr;

    const ONES_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const TWOS_HASH: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    const SIZE: u64 = 123;

    pub fn uri_builder(self_uri: &str, upstream_uri: Option<&str>) -> Result<UriBuilder, Error> {
        let server = ServerUris::new(self_uri, upstream_uri)?;
        Ok(UriBuilder {
            repository: "repo123".to_string(),
            server: Arc::new(server),
        })
    }

    pub struct TestContextBuilder {
        fb: FacebookInit,
        repo: BlobRepo,
        self_uri: String,
        upstream_uri: Option<String>,
    }

    impl TestContextBuilder {
        pub fn repo(mut self, repo: BlobRepo) -> Self {
            self.repo = repo;
            self
        }

        pub fn upstream_uri(mut self, upstream_uri: Option<String>) -> Self {
            self.upstream_uri = upstream_uri;
            self
        }

        pub fn build(self) -> Result<RepositoryRequestContext, Error> {
            let Self {
                fb,
                repo,
                self_uri,
                upstream_uri,
            } = self;

            let uri_builder = uri_builder(&self_uri, upstream_uri.as_deref())?;

            Ok(RepositoryRequestContext {
                ctx: CoreContext::test_mock(fb),
                repo,
                config: Arc::new(ServerConfig::default()),
                uri_builder,
                always_wait_for_upstream: false,
                max_upload_size: None,
                client: HttpClient::Disabled,
            })
        }
    }

    impl RepositoryRequestContext {
        pub fn test_builder(fb: FacebookInit) -> Result<TestContextBuilder, Error> {
            Ok(TestContextBuilder {
                fb,
                repo: TestRepoBuilder::new().build()?,
                self_uri: "http://foo.com/".to_string(),
                upstream_uri: Some("http://bar.com".to_string()),
            })
        }
    }

    fn obj() -> Result<RequestObject, Error> {
        Ok(RequestObject {
            oid: LfsSha256::from_str(ONES_HASH)?,
            size: SIZE,
        })
    }

    fn content_id() -> Result<ContentId, Error> {
        Ok(ContentId::from_str(ONES_HASH)?)
    }

    fn oid() -> Result<Sha256, Error> {
        Sha256::from_str(TWOS_HASH)
    }

    #[test]
    fn test_basic_upload_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com", Some("http://bar.com"))?;
        assert_eq!(
            b.upload_uri(&obj()?)?.to_string(),
            format!("http://foo.com/repo123/upload/{}/{}", ONES_HASH, SIZE),
        );
        Ok(())
    }

    #[test]
    fn test_basic_upload_uri_slash() -> Result<(), Error> {
        let b = uri_builder("http://foo.com/", Some("http://bar.com"))?;
        assert_eq!(
            b.upload_uri(&obj()?)?.to_string(),
            format!("http://foo.com/repo123/upload/{}/{}", ONES_HASH, SIZE),
        );
        Ok(())
    }

    #[test]
    fn test_prefix_upload_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com/bar", Some("http://bar.com"))?;
        assert_eq!(
            b.upload_uri(&obj()?)?.to_string(),
            format!("http://foo.com/bar/repo123/upload/{}/{}", ONES_HASH, SIZE),
        );
        Ok(())
    }

    #[test]
    fn test_prefix_slash_upload_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com/bar/", Some("http://bar.com"))?;
        assert_eq!(
            b.upload_uri(&obj()?)?.to_string(),
            format!("http://foo.com/bar/repo123/upload/{}/{}", ONES_HASH, SIZE),
        );
        Ok(())
    }

    #[test]
    fn test_basic_download_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com", Some("http://bar.com"))?;
        assert_eq!(
            b.download_uri(&content_id()?)?.to_string(),
            format!("http://foo.com/repo123/download/{}", ONES_HASH),
        );
        Ok(())
    }

    #[test]
    fn test_basic_download_uri_slash() -> Result<(), Error> {
        let b = uri_builder("http://foo.com/", Some("http://bar.com"))?;
        assert_eq!(
            b.download_uri(&content_id()?)?.to_string(),
            format!("http://foo.com/repo123/download/{}", ONES_HASH),
        );
        Ok(())
    }

    #[test]
    fn test_prefix_download_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com/bar", Some("http://bar.com"))?;
        assert_eq!(
            b.download_uri(&content_id()?)?.to_string(),
            format!("http://foo.com/bar/repo123/download/{}", ONES_HASH),
        );
        Ok(())
    }

    #[test]
    fn test_prefix_slash_download_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com/bar/", Some("http://bar.com"))?;
        assert_eq!(
            b.download_uri(&content_id()?)?.to_string(),
            format!("http://foo.com/bar/repo123/download/{}", ONES_HASH),
        );
        Ok(())
    }

    #[test]
    fn test_basic_consistent_download_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com", Some("http://bar.com"))?;
        assert_eq!(
            b.consistent_download_uri(&content_id()?, oid()?)?
                .to_string(),
            format!(
                "http://foo.com/repo123/download/{}?routing={}",
                ONES_HASH, TWOS_HASH
            ),
        );
        Ok(())
    }

    #[test]
    fn test_basic_upstream_batch_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com", Some("http://bar.com"))?;
        assert_eq!(
            b.upstream_batch_uri()?.map(|uri| uri.to_string()),
            Some(format!("http://bar.com/objects/batch")),
        );
        Ok(())
    }

    #[test]
    fn test_basic_upstream_batch_uri_slash() -> Result<(), Error> {
        let b = uri_builder("http://foo.com/", Some("http://bar.com"))?;
        assert_eq!(
            b.upstream_batch_uri()?.map(|uri| uri.to_string()),
            Some(format!("http://bar.com/objects/batch")),
        );
        Ok(())
    }

    #[test]
    fn test_prefix_upstream_batch_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com", Some("http://bar.com/foo"))?;
        assert_eq!(
            b.upstream_batch_uri()?.map(|uri| uri.to_string()),
            Some(format!("http://bar.com/foo/objects/batch")),
        );
        Ok(())
    }

    #[test]
    fn test_prefix_slash_upstream_batch_uri() -> Result<(), Error> {
        let b = uri_builder("http://foo.com", Some("http://bar.com/foo/"))?;
        assert_eq!(
            b.upstream_batch_uri()?.map(|uri| uri.to_string()),
            Some(format!("http://bar.com/foo/objects/batch")),
        );
        Ok(())
    }
}
