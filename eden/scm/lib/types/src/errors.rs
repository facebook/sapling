/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("Key Error: {0:?}")]
pub struct KeyError(#[source] Error);

impl KeyError {
    pub fn new(err: Error) -> Self {
        KeyError(err)
    }
}

/// NeworkError is a wrapper/tagging error meant for libraries to use
/// to mark errors that may imply a network problem.
#[derive(Debug, Error)]
#[error("Network Error: {0:?}")]
pub struct NetworkError(#[source] pub Error);

impl NetworkError {
    pub fn wrap(err: impl Into<Error>) -> Error {
        Self(err.into()).into()
    }
}

pub fn is_network_error(err: &Error) -> bool {
    has_in_chain::<NetworkError>(err)
}

fn has_in_chain<E>(err: &Error) -> bool
where
    E: std::error::Error + 'static,
{
    err.chain().any(|e| e.is::<E>())
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    #[test]
    fn test_network_error() {
        let network: &dyn std::error::Error =
            &NetworkError(io::Error::from(io::ErrorKind::Other).into());
        assert!(network.is::<NetworkError>());
        assert!(network.source().unwrap().is::<io::Error>());

        let network: Error = NetworkError(io::Error::from(io::ErrorKind::Other).into()).into();
        assert!(network.is::<NetworkError>());
        assert!(network.source().unwrap().is::<io::Error>());
        assert!(is_network_error(&network));

        assert_eq!(format!("{}", network), "Network Error: other error");

        let with_context = network.context("hello");
        assert!(is_network_error(&with_context));

        let wrapped: Error = KeyError(with_context).into();
        assert!(is_network_error(&wrapped));
    }
}
