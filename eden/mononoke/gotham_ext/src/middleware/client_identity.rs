/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use dns_lookup::lookup_addr;
use futures::Future;
use gotham::state::{client_addr, FromState, State};
use gotham_derive::StateData;
use hyper::header::HeaderMap;
use hyper::{Body, Response};
use lazy_static::lazy_static;
use percent_encoding::percent_decode;
use permission_checker::{MononokeIdentity, MononokeIdentitySet};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::task;

use super::Middleware;

use crate::socket_data::TlsCertificateIdentities;

const ENCODED_CLIENT_IDENTITY: &str = "x-fb-validated-client-encoded-identity";
const CLIENT_IP: &str = "tfb-orig-client-ip";
const CLIENT_CORRELATOR: &str = "x-client-correlator";

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

    /// Perform a reverse DNS lookup of the client's IP address to determine
    /// its hostname. This involves potentially expensive blocking I/O, so
    /// the lookup is performed asynchronously in another thread.
    pub fn hostname(&self) -> impl Future<Output = Option<String>> + 'static {
        // XXX: Can't make this an async fn because the resulting Future would
        // have a non-'static lifetime (due to the &self argument).
        let address = self.address.clone();
        async move {
            task::spawn_blocking(move || lookup_addr(&address?).ok())
                .await
                .ok()
                .flatten()
        }
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
    trusted_proxy_allowlist: Arc<MononokeIdentitySet>,
}

impl ClientIdentityMiddleware {
    pub fn new(trusted_proxy_idents: MononokeIdentitySet) -> Self {
        Self {
            trusted_proxy_allowlist: Arc::new(trusted_proxy_idents),
        }
    }

    fn extract_client_identities(
        &self,
        cert_idents: MononokeIdentitySet,
        headers: &HeaderMap,
    ) -> Option<MononokeIdentitySet> {
        let is_trusted_proxy = !self.trusted_proxy_allowlist.is_disjoint(&cert_idents);
        if is_trusted_proxy {
            request_identities_from_headers(&headers)
        } else {
            Some(cert_idents)
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

            if let Some(cert_idents) = cert_idents {
                client_identity.identities =
                    self.extract_client_identities(cert_idents.identities, &headers);
            }
        }

        // For the IP, we can fallback to the peer IP
        if client_identity.address.is_none() {
            client_identity.address = client_addr(&state).as_ref().map(SocketAddr::ip);
        }

        state.put(client_identity);

        None
    }
}
