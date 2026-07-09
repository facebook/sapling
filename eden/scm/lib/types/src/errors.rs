/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use anyhow::Error;
use thiserror::Error;

use crate::Key;

#[derive(Debug, Error)]
#[error("Key Error: {0:?}")]
pub struct KeyError(#[source] Error);

impl KeyError {
    pub fn new(err: Error) -> Self {
        KeyError(err)
    }
}

#[derive(Debug, Error)]
#[error("{0}: {1:#}")]
pub struct KeyedError(pub Key, #[source] pub Error);

#[derive(Clone)]
pub struct SharedError(Arc<Error>);

impl SharedError {
    pub fn new(err: Error) -> Self {
        Self(Arc::new(err))
    }

    pub fn into_anyhow(self) -> Error {
        match Arc::try_unwrap(self.0) {
            Ok(err) => err,
            Err(err) => Self(err).into(),
        }
    }
}

impl std::error::Error for SharedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref().as_ref())
    }
}

impl std::fmt::Display for SharedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self.0.as_ref(), f)
    }
}

impl std::fmt::Debug for SharedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.0.as_ref(), f)
    }
}

/// Error indicating that access to a tree was denied by a path ACL.
#[derive(Debug, Clone, PartialEq, Error)]
#[error(
    "permission denied: path '{path}' (tree {hgid}) is restricted (request access via ACL '{request_acl}')"
)]
pub struct PermissionDenied {
    pub path: crate::RepoPathBuf,
    pub hgid: crate::HgId,
    pub request_acl: String,
}

#[derive(Default)]
pub struct PermissionDeniedRecords {
    paths: Mutex<VecDeque<PermissionDenied>>,
    // Monotonic total of recorded errors. `paths` is a bounded recent-error
    // queue, so this is intentionally not the current queue length.
    count: AtomicU64,
}

impl PermissionDeniedRecords {
    pub fn record(&self, err: PermissionDenied) {
        self.count.fetch_add(1, Ordering::Relaxed);
        let mut paths = self.lock();
        if paths.len() >= 1000 {
            paths.pop_front();
        }
        paths.push_back(err);
    }

    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    pub fn lock(&self) -> MutexGuard<'_, VecDeque<PermissionDenied>> {
        self.paths.lock().expect("permission denied paths poisoned")
    }
}

pub type PermissionDeniedPaths = Arc<PermissionDeniedRecords>;

/// NetworkError is a wrapper/tagging error meant for libraries to use
/// to mark errors that may imply a network problem.
pub struct NetworkError(pub Error);

impl std::error::Error for NetworkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

impl std::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Network Error: {}", self.0)
    }
}

impl std::fmt::Debug for NetworkError {
    // This normally is not called since anyhow Debug impl does not call underlying error's Debug.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Network Error: {:?}", self.0)
    }
}

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

    use anyhow::anyhow;

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

        assert_eq!(format!("{network}"), "Network Error: other error");

        let with_context = network.context("hello");
        assert!(is_network_error(&with_context));

        let wrapped: Error = KeyError(with_context).into();
        assert!(is_network_error(&wrapped));
    }

    #[test]
    fn test_debug_output() {
        let inner_error = anyhow!(io::Error::from(io::ErrorKind::Other)).context("some context");
        let network = NetworkError::wrap(inner_error);

        assert_eq!(
            format!("{network:?}"),
            "Network Error: some context

Caused by:
    0: some context
    1: other error"
        );
    }
}
