/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use aclchecker::Identity;
use bytes::Bytes;
use gotham::{state::State, PreStateData};
use gotham_derive::StateData;
use openssl::ssl::SslRef;
use x509::identity;

pub struct TlsPreStateData {
    identities: Option<TlsCertificateIdentities>,
    session_data: Option<TlsSessionData>,
}

impl TlsPreStateData {
    pub fn from_ssl(ssl: &SslRef, capture_session_data: bool) -> Self {
        let identities = TlsCertificateIdentities::from_ssl(ssl);

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
}

impl PreStateData for TlsPreStateData {
    fn fill_state(&self, state: &mut State) {
        if let Some(ref identities) = self.identities {
            state.put(identities.clone());
        }

        if let Some(ref session_data) = self.session_data {
            state.put(session_data.clone());
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
pub struct TlsCertificateIdentities {
    pub identities: Vec<Identity>,
}

impl TlsCertificateIdentities {
    pub fn from_ssl(ssl: &SslRef) -> Option<Self> {
        let peer_certificate = ssl.peer_certificate()?;
        let identities = identity::get_identities(&peer_certificate).ok()?;
        Some(Self { identities })
    }
}
