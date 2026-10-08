/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use try_once_lock::OnceLock;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedAuth {
    pub cert_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>,
    pub ca_path: Option<PathBuf>,
}

type ResolveAuth = dyn Fn() -> anyhow::Result<ResolvedAuth> + Send + Sync;

struct AuthResolverInner {
    resolve: Box<ResolveAuth>,
    resolved: OnceLock<ResolvedAuth>,
}

/// Resolves HTTP authentication paths, caching the first successful result.
#[derive(Clone)]
pub struct AuthResolver(Arc<AuthResolverInner>);

impl AuthResolver {
    pub fn new<F, E>(resolve: F) -> Self
    where
        F: Fn() -> Result<ResolvedAuth, E> + Send + Sync + 'static,
        E: Into<anyhow::Error>,
    {
        Self(Arc::new(AuthResolverInner {
            resolve: Box::new(move || resolve().map_err(Into::into)),
            resolved: OnceLock::new(),
        }))
    }

    pub fn resolve(&self) -> anyhow::Result<&ResolvedAuth> {
        self.0.resolved.get_or_try_init(|| (self.0.resolve)())
    }
}

impl fmt::Debug for AuthResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthResolver")
            .field("resolved", &self.0.resolved.get().is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::Relaxed;

    use super::*;

    #[test]
    fn failures_are_retried_and_first_success_is_cached() {
        let calls = Arc::new(AtomicUsize::new(0));
        let expected = ResolvedAuth {
            cert_path: Some("cert.pem".into()),
            key_path: Some("key.pem".into()),
            ca_path: Some("ca.pem".into()),
        };
        let resolver = AuthResolver::new({
            let calls = calls.clone();
            let expected = expected.clone();
            move || {
                if calls.fetch_add(1, Relaxed) == 0 {
                    Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "certificate is not available yet",
                    ))
                } else {
                    Ok(expected.clone())
                }
            }
        });

        assert!(resolver.resolve().is_err());
        assert_eq!(resolver.resolve().unwrap(), &expected);
        assert_eq!(resolver.resolve().unwrap(), &expected);
        assert_eq!(calls.load(Relaxed), 2);
    }
}
