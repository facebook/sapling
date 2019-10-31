/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use aclchecker::Identity;
use gotham::state::{client_addr, FromState, State};
use gotham_derive::StateData;
use hyper::header::HeaderMap;
use json_encoded::get_identities;
use lazy_static::lazy_static;
use percent_encoding::percent_decode;
use std::net::{IpAddr, SocketAddr};

use super::Middleware;

const ENCODED_CLIENT_IDENTITY: &str = "x-fb-validated-client-encoded-identity";
const CLIENT_IP: &str = "tfb-orig-client-ip";
const CLIENT_CORRELATOR: &str = "x-client-correlator";

lazy_static! {
    static ref PROXYGEN_ORIGIN_IDENTITY: Identity =
        Identity::new("SERVICE_IDENTITY", "proxygen-origin");
}

#[derive(StateData, Default)]
pub struct ClientIdentity {
    address: Option<IpAddr>,
    identities: Option<Vec<Identity>>,
    client_correlator: Option<String>,
}

impl ClientIdentity {
    pub fn address(&self) -> &Option<IpAddr> {
        &self.address
    }

    pub fn identities(&self) -> &Option<Vec<Identity>> {
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
pub struct ClientIdentityMiddleware {}

impl ClientIdentityMiddleware {
    pub fn new() -> Self {
        Self {}
    }
}

fn request_ip_from_headers(headers: &HeaderMap) -> Option<IpAddr> {
    let header = headers.get(CLIENT_IP)?;
    let header = header.to_str().ok()?;
    let ip = header.parse().ok()?;
    Some(ip)
}

fn request_identities_from_headers(headers: &HeaderMap) -> Option<Vec<Identity>> {
    let encoded_identities = headers.get(ENCODED_CLIENT_IDENTITY)?;
    let json_identities = percent_decode(encoded_identities.as_bytes())
        .decode_utf8()
        .ok()?;
    let identities = get_identities(&json_identities).ok()?;
    Some(identities)
}

fn request_client_correlator_from_headers(headers: &HeaderMap) -> Option<String> {
    let header = headers.get(CLIENT_CORRELATOR)?;
    let header = header.to_str().ok()?;
    Some(header.to_string())
}

impl Middleware for ClientIdentityMiddleware {
    fn inbound(&self, state: &mut State) {
        let mut client_identity = ClientIdentity::default();

        if let Some(headers) = HeaderMap::try_borrow_from(&state) {
            client_identity.address = request_ip_from_headers(&headers);
            client_identity.identities = request_identities_from_headers(&headers);
            client_identity.client_correlator = request_client_correlator_from_headers(&headers);
        }

        // For the IP, we can fallback to the peer IP
        if client_identity.address.is_none() {
            client_identity.address = client_addr(&state).as_ref().map(SocketAddr::ip);
        }

        state.put(client_identity);
    }
}
