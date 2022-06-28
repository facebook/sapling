/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use gotham::state::State;
use gotham_derive::StateData;
use openssl::ssl::SslRef;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;

#[derive(Clone)]
pub struct TlsSocketData {
    identities: Option<TlsCertificateIdentities>,
    session_data: Option<TlsSessionData>,
}

impl TlsSocketData {
    pub fn from_ssl(
        ssl: &SslRef,
        trusted_proxy_allowlist: &MononokeIdentitySet,
        capture_session_data: bool,
    ) -> Self {
        let identities = TlsCertificateIdentities::from_ssl(ssl, trusted_proxy_allowlist);

        let session_data = if capture_session_data {
            TlsSessionData::from_ssl(ssl)
        } else {
            None
        };

        Self {
            identities,
            session_data,
        }
    }

    pub fn trusted_proxy() -> Self {
        Self {
            identities: Some(TlsCertificateIdentities::TrustedProxy),
            session_data: None,
        }
    }

    pub fn authenticated_identities(identities: MononokeIdentitySet) -> Self {
        Self {
            identities: Some(TlsCertificateIdentities::Authenticated(identities)),
            session_data: None,
        }
    }

    pub fn populate_state(self, state: &mut State) {
        if let Some(identities) = self.identities {
            state.put(identities);
        }

        if let Some(session_data) = self.session_data {
            state.put(session_data);
        }
    }
}

#[derive(Clone, StateData)]
pub struct TlsSessionData {
    pub client_random: Bytes,
    pub master_key: Bytes,
}

impl TlsSessionData {
    pub fn from_ssl(ssl: &SslRef) -> Option<Self> {
        let session = ssl.session()?;

        // NOTE: The OpenSSL API for getting session data is that you pass a zero-sized destination
        // to get the size. This is why we do this here.
        let mut empty: [u8; 0] = [];

        // NOTE: We use assert_eq! below, because it would be a programming error to receive less
        // than the proper size back here.

        let client_random_len = ssl.client_random(&mut empty);
        let mut client_random = vec![0; client_random_len];
        assert_eq!(client_random_len, ssl.client_random(&mut client_random[..]));

        let master_key_len = session.master_key(&mut empty);
        let mut master_key = vec![0; master_key_len];
        assert_eq!(master_key_len, session.master_key(&mut master_key[..]));

        Some(Self {
            client_random: client_random.into(),
            master_key: master_key.into(),
        })
    }
}

#[derive(Clone, StateData)]
pub enum TlsCertificateIdentities {
    TrustedProxy,
    Authenticated(MononokeIdentitySet),
}

impl TlsCertificateIdentities {
    pub fn from_ssl(ssl: &SslRef, trusted_proxy_allowlist: &MononokeIdentitySet) -> Option<Self> {
        let peer_certificate = ssl.peer_certificate()?;
        let identities = MononokeIdentity::try_from_x509(&peer_certificate).ok()?;

        let ret = if trusted_proxy_allowlist.is_disjoint(&identities) {
            TlsCertificateIdentities::Authenticated(identities)
        } else {
            TlsCertificateIdentities::TrustedProxy
        };

        Some(ret)
    }
}
