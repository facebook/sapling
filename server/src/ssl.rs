// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use openssl::ssl::{SslAcceptor, SslMethod, SslVerifyMode};

use secure_utils;

use errors::*;

pub struct SslConfig {
    pub cert: String,
    pub private_key: String,
    pub ca_pem: String,
}

// Builds an acceptor that has `accept_async()` method that handles tls handshake
// and returns decrypted stream.
pub fn build_tls_acceptor(ssl: SslConfig) -> Result<SslAcceptor> {
    let mut acceptor = SslAcceptor::mozilla_intermediate(SslMethod::tls())?;

    let pkcs12 =
        secure_utils::build_identity(ssl.cert, ssl.private_key).context("failed to build pkcs12")?;
    acceptor.set_certificate(&pkcs12.cert)?;
    acceptor.set_private_key(&pkcs12.pkey)?;

    // Set up client authentication via root certificate
    acceptor
        .cert_store_mut()
        .add_cert(secure_utils::read_x509(ssl.ca_pem)?)?;
    acceptor.set_verify(SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT);

    Ok(acceptor.build())
}
