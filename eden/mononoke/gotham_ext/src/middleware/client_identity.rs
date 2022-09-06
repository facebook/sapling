/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::net::IpAddr;
use std::net::SocketAddr;

use cats::try_get_cats_idents;
use fbinit::FacebookInit;
use futures::FutureExt;
use gotham::state::client_addr;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use hyper::header::HeaderMap;
use hyper::Body;
use hyper::Response;
use hyper::StatusCode;
use metaconfig_types::Identity;
use percent_encoding::percent_decode;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;
use slog::error;
use slog::Logger;

use super::Middleware;
use crate::socket_data::TlsCertificateIdentities;
use crate::state_ext::StateExt;

const ENCODED_CLIENT_IDENTITY: &str = "x-fb-validated-client-encoded-identity";
const CLIENT_IP: &str = "tfb-orig-client-ip";

#[derive(StateData, Default)]
pub struct ClientIdentity {
    address: Option<IpAddr>,
    identities: Option<MononokeIdentitySet>,
}

impl ClientIdentity {
    pub fn address(&self) -> &Option<IpAddr> {
        &self.address
    }

    pub fn identities(&self) -> &Option<MononokeIdentitySet> {
        &self.identities
    }
}

#[derive(Clone)]
pub struct ClientIdentityMiddleware {
    fb: FacebookInit,
    logger: Logger,
    internal_identity: Identity,
}

impl ClientIdentityMiddleware {
    pub fn new(fb: FacebookInit, logger: Logger, internal_identity: Identity) -> Self {
        Self {
            fb,
            logger,
            internal_identity,
        }
    }

    fn extract_client_identities(
        &self,
        tls_certificate_identities: TlsCertificateIdentities,
        headers: &HeaderMap,
    ) -> Option<MononokeIdentitySet> {
        match tls_certificate_identities {
            TlsCertificateIdentities::TrustedProxy => request_identities_from_headers(headers),
            TlsCertificateIdentities::Authenticated(idents) => Some(idents),
        }
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

#[async_trait::async_trait]
impl Middleware for ClientIdentityMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        let mut client_identity = ClientIdentity::default();
        let cert_idents = TlsCertificateIdentities::try_take_from(state);

        if let Some(headers) = HeaderMap::try_borrow_from(state) {
            client_identity.address = request_ip_from_headers(headers);

            client_identity.identities = {
                let maybe_cat_idents =
                    match try_get_cats_idents(self.fb, headers, &self.internal_identity) {
                        Err(e) => {
                            let msg = format!("Error extracting CATs identities: {}.", &e,);
                            error!(self.logger, "{}", &msg,);
                            let response = Response::builder()
                                .status(StatusCode::UNAUTHORIZED)
                                .body(
                                    format!(
                                        "{{\"message:\"{}\", \"request_id\":\"{}\"}}",
                                        msg,
                                        state.short_request_id()
                                    )
                                    .into(),
                                )
                                .expect("Couldn't build http response");

                            return Some(response);
                        }
                        Ok(maybe_cats) => maybe_cats,
                    };

                let maybe_tls_or_proxied_idents: Option<MononokeIdentitySet> =
                    cert_idents.and_then(|x| self.extract_client_identities(x, headers));

                match (maybe_cat_idents, maybe_tls_or_proxied_idents) {
                    (None, None) => None,
                    (Some(cat_idents), Some(tls_or_proxied_idents)) => {
                        Some(cat_idents.union(&tls_or_proxied_idents).cloned().collect())
                    }
                    (Some(cat_idents), None) => Some(cat_idents),
                    (None, Some(tls_or_proxied_idents)) => Some(tls_or_proxied_idents),
                }
            };
        }

        // For the IP, we can fallback to the peer IP
        if client_identity.address.is_none() {
            client_identity.address = client_addr(state).as_ref().map(SocketAddr::ip);
        }

        state.put(client_identity);

        None
    }
}
