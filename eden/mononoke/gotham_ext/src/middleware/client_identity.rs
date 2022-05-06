/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Error};
use fbinit::FacebookInit;
use futures::{future, Future, FutureExt};
use gotham::state::{client_addr, FromState, State};
use gotham_derive::StateData;
use hyper::header::HeaderMap;
use hyper::{Body, Response};
use lazy_static::lazy_static;
use percent_encoding::percent_decode;
use permission_checker::{MononokeIdentity, MononokeIdentitySet, MononokeIdentitySetExt};
use slog::{error, Logger};
use std::net::{IpAddr, SocketAddr};
use trust_dns_resolver::TokioAsyncResolver;

use super::Middleware;

use crate::socket_data::TlsCertificateIdentities;

const ENCODED_CLIENT_IDENTITY: &str = "x-fb-validated-client-encoded-identity";
const CLIENT_IP: &str = "tfb-orig-client-ip";
const CLIENT_CORRELATOR: &str = "x-client-correlator";
const HEADER_CRYPTO_AUTH_TOKENS: &str = "x-auth-cats";

lazy_static! {
    static ref PROXYGEN_ORIGIN_IDENTITY: MononokeIdentity =
        MononokeIdentity::new("SERVICE_IDENTITY", "proxygen-origin")
            .expect("SERVICE_IDENTITY is not a valid identity type");
}

#[derive(StateData, Default)]
pub struct ClientIdentity {
    address: Option<IpAddr>,
    identities: Option<MononokeIdentitySet>,
    client_correlator: Option<String>,
}

impl ClientIdentity {
    pub fn address(&self) -> &Option<IpAddr> {
        &self.address
    }

    // Hostname of the client is for non-critical use only (best-effort lookup):
    pub fn hostname(&self) -> impl Future<Output = Option<String>> + 'static {
        // XXX: Can't make this an async fn because the resulting Future would
        // have a non-'static lifetime (due to the &self argument).

        // 1) We're extracting it from identities (which requires no remote calls)
        if let Some(client_hostname) = self
            .identities
            .as_ref()
            .map(|id| id.hostname().map(|h| h.to_string()))
            .flatten()
        {
            return future::ready(Some(client_hostname)).left_future();
        }
        // 2) Perform a reverse DNS lookup of the client's IP address to determine
        // its hostname.
        let address = self.address.clone();
        (async move {
            let resolver = TokioAsyncResolver::tokio_from_system_conf().ok()?;
            let hosts = resolver.reverse_lookup(address?).await.ok()?;
            let host = hosts.iter().next()?;
            Some(host.to_string().trim_end_matches('.').to_string())
        })
        .right_future()
    }

    // Extract the client's username from the identity set, if present.
    pub fn username(&self) -> Option<&str> {
        for id in self.identities.as_ref()? {
            if id.id_type() == "USER" {
                return Some(id.id_data());
            }
        }
        None
    }

    pub fn identities(&self) -> &Option<MononokeIdentitySet> {
        &self.identities
    }

    pub fn client_correlator(&self) -> &Option<String> {
        &self.client_correlator
    }

    pub fn is_proxygen_test_identity(&self) -> bool {
        if let Some(identities) = &self.identities {
            identities.contains(&PROXYGEN_ORIGIN_IDENTITY)
        } else {
            false
        }
    }
}

#[derive(Clone)]
pub struct ClientIdentityMiddleware {
    fb: FacebookInit,
    logger: Logger,
}

impl ClientIdentityMiddleware {
    pub fn new(fb: FacebookInit, logger: Logger) -> Self {
        Self { fb, logger }
    }

    fn extract_client_identities(
        &self,
        tls_certificate_identities: TlsCertificateIdentities,
        headers: &HeaderMap,
    ) -> Option<MononokeIdentitySet> {
        match tls_certificate_identities {
            TlsCertificateIdentities::TrustedProxy => request_identities_from_headers(&headers),
            TlsCertificateIdentities::Authenticated(idents) => Some(idents),
        }
    }

    #[cfg(not(fbcode_build))]
    fn try_get_cats_idents(
        &self,
        _headers: &HeaderMap,
    ) -> Result<Option<MononokeIdentitySet>, Error> {
        Ok(None)
    }

    #[cfg(fbcode_build)]
    fn try_get_cats_idents(
        &self,
        headers: &HeaderMap,
    ) -> Result<Option<MononokeIdentitySet>, Error> {
        let cats = match headers.get(HEADER_CRYPTO_AUTH_TOKENS) {
            Some(cats) => cats,
            None => return Ok(None),
        };

        let s_cats = cats.to_str()?;
        let cat_list = cryptocat::deserialize_crypto_auth_tokens(s_cats)?;
        let svc_scm_ident = cryptocat::Identity {
            id_type: "SERVICE_IDENTITY".to_string(),
            id_data: "scm_service_identity".to_string(),
            ..Default::default()
        };

        cat_list
            .tokens
            .into_iter()
            .try_fold(MononokeIdentitySet::new(), |mut idents_acc, token| {
                let tdata = cryptocat::deserialize_crypto_auth_token_data(
                    &token.serializedCryptoAuthTokenData[..],
                )?;
                let m_ident = MononokeIdentity::new(
                    tdata.signerIdentity.id_type,
                    tdata.signerIdentity.id_data,
                )?;
                idents_acc.insert(m_ident);
                let res =
                    cryptocat::verify_crypto_auth_token(self.fb, token, &svc_scm_ident, None)?;
                if res.code != cryptocat::CATVerificationCode::SUCCESS {
                    bail!(
                        "verification of CATs not successful. status code: {:?}",
                        res.code
                    );
                }
                Ok(idents_acc)
            })
            .map(Option::Some)
    }
}

fn request_ip_from_headers(headers: &HeaderMap) -> Option<IpAddr> {
    let header = headers.get(CLIENT_IP)?;
    let header = header.to_str().ok()?;
    let ip = header.parse().ok()?;
    Some(ip)
}

fn request_identities_from_headers(headers: &HeaderMap) -> Option<MononokeIdentitySet> {
    let encoded_identities = headers.get(ENCODED_CLIENT_IDENTITY)?;
    let json_identities = percent_decode(encoded_identities.as_bytes())
        .decode_utf8()
        .ok()?;
    MononokeIdentity::try_from_json_encoded(&json_identities).ok()
}

fn request_client_correlator_from_headers(headers: &HeaderMap) -> Option<String> {
    let header = headers.get(CLIENT_CORRELATOR)?;
    let header = header.to_str().ok()?;
    Some(header.to_string())
}

#[async_trait::async_trait]
impl Middleware for ClientIdentityMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        let mut client_identity = ClientIdentity::default();
        let cert_idents = TlsCertificateIdentities::try_take_from(state);

        if let Some(headers) = HeaderMap::try_borrow_from(&state) {
            client_identity.address = request_ip_from_headers(&headers);
            client_identity.client_correlator = request_client_correlator_from_headers(&headers);

            client_identity.identities = {
                // Ideally, the logging would be in an `inspect_err` closure, but that's experimental
                // and then we'd use `unwrap_or_default()` to get `None` into maybe_idents.
                let maybe_idents = self.try_get_cats_idents(headers).unwrap_or_else(|e| {
                    error!(
                        self.logger,
                        "Error extracting CATs identities: {}. Falling back to other auth methods",
                        &e
                    );
                    None
                });
                maybe_idents.or_else(|| {
                    cert_idents.and_then(|x| self.extract_client_identities(x, headers))
                })
            };
        }

        // For the IP, we can fallback to the peer IP
        if client_identity.address.is_none() {
            client_identity.address = client_addr(&state).as_ref().map(SocketAddr::ip);
        }

        state.put(client_identity);

        None
    }
}
