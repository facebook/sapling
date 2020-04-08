/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::Blobstore;
use cloned::cloned;
use context::CoreContext;
use futures::{compat::Future01CompatExt, FutureExt as _, TryFutureExt};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::BlobstoreBytes;
use scopeguard::defer;

/// A layer over an existing blobstore that respects a CoreContext's blobstore concurrency
#[derive(Clone, Debug)]
pub struct ContextConcurrencyBlobstore<T: Blobstore + Clone> {
    blobstore: T,
}

impl<T: Blobstore + Clone> ContextConcurrencyBlobstore<T> {
    pub fn new(blobstore: T) -> Self {
        Self { blobstore }
    }

    pub fn as_inner(&self) -> &T {
        &self.blobstore
    }

    pub fn into_inner(self) -> T {
        self.blobstore
    }
}

impl<T: Blobstore + Clone> Blobstore for ContextConcurrencyBlobstore<T> {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        cloned!(self.blobstore);
        async move {
            // NOTE: We need to clone() here because the context cannot be borrowed when we pass it
            // down. We should eventually be able to get rid of this.
            let session = ctx.session().clone();

            let permit = match session.blobstore_semaphore() {
                Some(sem) => Some(sem.acquire().await),
                None => None,
            };

            defer!({
                drop(permit);
            });

            blobstore.get(ctx, key).compat().await
        }
        .boxed()
        .compat()
        .boxify()
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        cloned!(self.blobstore);
        async move {
            let session = ctx.session().clone();

            let permit = match session.blobstore_semaphore() {
                Some(sem) => Some(sem.acquire().await),
                None => None,
            };

            defer!({
                drop(permit);
            });

            blobstore.put(ctx, key, value).compat().await
        }
        .boxed()
        .compat()
        .boxify()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        cloned!(self.blobstore);
        async move {
            let session = ctx.session().clone();

            let permit = match session.blobstore_semaphore() {
                Some(sem) => Some(sem.acquire().await),
                None => None,
            };

            defer!({
                drop(permit);
            });

            blobstore.is_present(ctx, key).compat().await
        }
        .boxed()
        .compat()
        .boxify()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use context::SessionContainer;
    use fbinit::FacebookInit;
    use scuba_ext::ScubaSampleBuilder;
    use slog::{o, Drain, Level, Logger};
    use slog_glog_fmt::default_drain;
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    };
    use std::time::Duration;
    use tokio::time;

    #[derive(Clone, Debug)]
    struct NonConcurentBlobstore(Arc<AtomicU64>);

    impl NonConcurentBlobstore {
        fn new() -> Self {
            Self(Arc::new(AtomicU64::new(0)))
        }
    }

    impl Blobstore for NonConcurentBlobstore {
        fn get(&self, _ctx: CoreContext, _key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
            let ctr = self.0.clone();
            if ctr.fetch_add(1, Ordering::Relaxed) > 0 {
                panic!("No!");
            }

            async move {
                time::delay_for(Duration::from_millis(10)).await;
                ctr.fetch_sub(1, Ordering::Relaxed);
                Ok(None)
            }
            .boxed()
            .compat()
            .boxify()
        }

        fn put(
            &self,
            _ctx: CoreContext,
            _key: String,
            _value: BlobstoreBytes,
        ) -> BoxFuture<(), Error> {
            let ctr = self.0.clone();
            if self.0.fetch_add(1, Ordering::Relaxed) > 0 {
                panic!("No!");
            }

            async move {
                time::delay_for(Duration::from_millis(10)).await;
                ctr.fetch_sub(1, Ordering::Relaxed);
                Ok(())
            }
            .boxed()
            .compat()
            .boxify()
        }

        fn is_present(&self, _ctx: CoreContext, _key: String) -> BoxFuture<bool, Error> {
            let ctr = self.0.clone();
            if self.0.fetch_add(1, Ordering::Relaxed) > 0 {
                panic!("No!");
            }

            async move {
                ctr.fetch_sub(1, Ordering::Relaxed);
                time::delay_for(Duration::from_millis(10)).await;
                Ok(false)
            }
            .boxed()
            .compat()
            .boxify()
        }
    }

    fn logger() -> Logger {
        let drain = default_drain().filter_level(Level::Debug).ignore_res();
        Logger::root(drain, o![])
    }

    #[fbinit::test]
    async fn test_semaphore(fb: FacebookInit) -> Result<(), Error> {
        let session = SessionContainer::builder(fb)
            .blobstore_concurrency(1)
            .build();
        let ctx = session.new_context(logger(), ScubaSampleBuilder::with_discard());

        let blob = ContextConcurrencyBlobstore::new(NonConcurentBlobstore::new());

        let res = futures::future::try_join(
            blob.get(ctx.clone(), "foo".to_string()).compat(),
            blob.get(ctx.clone(), "foo".to_string()).compat(),
        )
        .await?;
        assert_eq!(res, (None, None));

        let bytes = BlobstoreBytes::from_bytes("test foobar");
        let res = futures::future::try_join(
            blob.put(ctx.clone(), "foo".to_string(), bytes.clone())
                .compat(),
            blob.put(ctx.clone(), "foo".to_string(), bytes.clone())
                .compat(),
        )
        .await?;
        assert_eq!(res, ((), ()));

        let res = futures::future::try_join(
            blob.is_present(ctx.clone(), "foo".to_string()).compat(),
            blob.is_present(ctx.clone(), "foo".to_string()).compat(),
        )
        .await?;
        assert_eq!(res, (false, false));

        Ok(())
    }
}
