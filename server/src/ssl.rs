// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use native_tls::TlsAcceptor;
use native_tls::backend::openssl::TlsAcceptorBuilderExt;
use openssl::ssl::{SSL_VERIFY_FAIL_IF_NO_PEER_CERT, SSL_VERIFY_PEER};

use secure_utils;

use errors::*;

pub struct SslConfig {
    pub cert: String,
    pub private_key: String,
    pub ca_pem: String,
}

// Builds an acceptor that has `accept_async()` method that handles tls handshake
// and returns decrypted stream.
pub fn build_tls_acceptor(ssl: SslConfig) -> Result<TlsAcceptor> {
    let pkcs12 =
        secure_utils::build_pkcs12(ssl.cert, ssl.private_key).context("failed to build pkcs12")?;
    let mut tlsacceptor_builder = TlsAcceptor::builder(pkcs12)?;

    // Set up client authentication
    {
        let sslcontextbuilder = tlsacceptor_builder.builder_mut();

        sslcontextbuilder
            .set_ca_file(ssl.ca_pem)
            .context("cannot set CA file")?;

        // SSL_VERIFY_PEER checks client certificate if it was supplied.
        // Connection is terminated if certificate verification fails.
        // SSL_VERIFY_FAIL_IF_NO_PEER_CERT terminates the connection if client did not return
        // certificate.
        // More about it - https://wiki.openssl.org/index.php/Manual:SSL_CTX_set_verify(3)
        sslcontextbuilder.set_verify(SSL_VERIFY_PEER | SSL_VERIFY_FAIL_IF_NO_PEER_CERT);
    }
    tlsacceptor_builder.build().map_err(Error::from)
}
