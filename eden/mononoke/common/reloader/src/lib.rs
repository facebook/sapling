/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(fn_traits)]

use anyhow::anyhow;
use anyhow::Result;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use cloned::cloned;
use context::CoreContext;
use futures::select;
use futures::FutureExt;
use futures_ext::future::spawn_controlled;
use futures_ext::future::ControlledHandle;
use rand::Rng;
use slog::warn;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

#[derive(Debug, Clone)]
pub struct Reloader<R> {
    obj: Arc<ArcSwap<R>>,
    _handle: Option<ControlledHandle>,
    notify: Arc<Notify>,
}

impl<R> Reloader<R> {
    pub fn load(&self) -> arc_swap::Guard<Arc<R>> {
        self.obj.load()
    }

    pub fn load_full(&self) -> Arc<R> {
        self.obj.load_full()
    }

    pub fn fixed(r: R) -> Self {
        Self {
            obj: Arc::new(ArcSwap::from_pointee(r)),
            _handle: None,
            notify: Arc::new(Notify::new()),
        }
    }

    // Only used in tests
    #[allow(dead_code)]
    pub async fn wait_for_update(&self) {
        self.notify.notified().await;
    }
}

#[async_trait]
pub trait Loader<R> {
    // Load a new instance of R.
    async fn load(&mut self) -> Result<Option<R>>;

    // Determines if R should be reloaded. This function is called to determine
    // if periodic reload is necessary. It's ignored by forced reload.
    async fn needs_reload(&mut self) -> Result<bool> {
        Ok(true)
    }
}

#[async_trait]
impl<R, O, F> Loader<R> for F
where
    O: Future<Output = Result<Option<R>>> + Send,
    F: FnMut() -> O + Send,
{
    async fn load(&mut self) -> Result<Option<R>> {
        self.call_mut(()).await
    }
}

impl<R: 'static + Send + Sync> Reloader<R> {
    pub async fn reload_periodically<
        I: 'static + FnMut() -> Duration + Send,
        L: 'static + Loader<R> + Send + Sync,
    >(
        ctx: CoreContext,
        interval_getter: I,
        loader: L,
    ) -> Result<Self> {
        Self::reload_periodically_with_force_reload(
            ctx,
            interval_getter,
            loader,
            Arc::new(Notify::new()),
        )
        .await
    }

    pub async fn reload_periodically_with_force_reload<
        I: 'static + FnMut() -> Duration + Send,
        L: 'static + Loader<R> + Send + Sync,
    >(
        ctx: CoreContext,
        mut interval_getter: I,
        mut loader: L,
        force_reload: Arc<Notify>,
    ) -> Result<Self> {
        let obj = Arc::new(ArcSwap::from_pointee(
            loader
                .load()
                .await?
                .ok_or_else(|| anyhow!("Missing initial object"))?,
        ));
        let notify = Arc::new(Notify::new());
        let handle = spawn_controlled({
            cloned!(obj, notify);
            async move {
                loop {
                    let interval = interval_getter();
                    let sleep_fut = tokio::time::sleep(interval).fuse();
                    let force_reload_fut = force_reload.notified().fuse();
                    futures::pin_mut!(sleep_fut, force_reload_fut);
                    let forced_reload = select! {
                        _ = sleep_fut => false,
                        _ = force_reload_fut => true,
                    };
                    let reload_now = if forced_reload {
                        true
                    } else {
                        loader.needs_reload().await.unwrap_or_else(|err| {
                            warn!(
                                ctx.logger(),
                                "Failed to check if reload needed, not reloading: {:?}", err
                            );
                            // I's better to keep a known-good SC than to reload an out-of-date version
                            // and catch up again.
                            //
                            // Since the check for an update failed, keep the current version.
                            false
                        })
                    };
                    if reload_now {
                        match loader.load().await {
                            Ok(Some(new)) => obj.store(Arc::new(new)),
                            // Fetch was successful, but there's nothing to reload
                            Ok(None) => {}
                            Err(err) => {
                                warn!(ctx.logger(), "Failed to reload: {:?}", err)
                            }
                        }
                        notify.notify_waiters();
                    }
                }
            }
        });
        Ok(Self {
            obj,
            _handle: Some(handle),
            notify,
        })
    }

    /// Reload periodically with a fixed interval, but on the first wait, randomly wait
    /// up to 10% more. Useful for preventing reloads at the same time when creating multiple
    /// reloaders at the same time with the same interval.
    pub async fn reload_periodically_with_skew<L: 'static + Loader<R> + Send + Sync>(
        ctx: CoreContext,
        period: Duration,
        loader: L,
    ) -> Result<Self> {
        Self::reload_periodically_with_skew_and_force_reload(
            ctx,
            period,
            loader,
            Arc::new(Notify::new()),
        )
        .await
    }

    pub async fn reload_periodically_with_skew_and_force_reload<
        L: 'static + Loader<R> + Send + Sync,
    >(
        ctx: CoreContext,
        period: Duration,
        loader: L,
        force_reload: Arc<Notify>,
    ) -> Result<Self> {
        let mut first = true;
        Self::reload_periodically_with_force_reload(
            ctx,
            move || {
                if first {
                    first = false;
                    let jitter = rand::thread_rng().gen_range(Duration::from_secs(0)..period / 10);
                    period + jitter
                } else {
                    period
                }
            },
            loader,
            force_reload,
        )
        .await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering::Relaxed;
    use std::time::Duration;

    #[test]
    fn test_fixed() {
        let l = Reloader::fixed(12);
        assert_eq!(**l.load(), 12);
        assert_eq!(**l.load(), 12);
    }

    #[fbinit::test]
    async fn test_reload(fb: FacebookInit) {
        tokio::time::pause();
        struct NumberLoader {
            cur: u32,
        }
        #[async_trait]
        impl Loader<u32> for NumberLoader {
            async fn load(&mut self) -> Result<Option<u32>> {
                let last = self.cur;
                self.cur += 1;
                Ok(Some(last))
            }
        }
        let loader = NumberLoader { cur: 0 };
        let l = Reloader::reload_periodically(
            CoreContext::test_mock(fb),
            || Duration::from_millis(20),
            loader,
        )
        .await
        .unwrap();

        assert_eq!(**l.load(), 0);
        tokio::time::advance(Duration::from_millis(21)).await;
        l.wait_for_update().await;
        assert_eq!(**l.load(), 1);
        tokio::time::advance(Duration::from_millis(21)).await;
        l.wait_for_update().await;
        assert_eq!(**l.load(), 2);
    }

    #[fbinit::test]
    async fn test_reload_fail_then_succeed(fb: FacebookInit) {
        tokio::time::pause();
        struct NumberLoader {
            cur: AtomicU32,
            failing: AtomicBool,
        }
        #[async_trait]
        impl Loader<u32> for Arc<NumberLoader> {
            async fn load(&mut self) -> Result<Option<u32>> {
                if self.failing.load(Relaxed) {
                    Err(anyhow!("Error"))
                } else {
                    Ok(Some(self.cur.fetch_add(1, Relaxed)))
                }
            }
        }
        let loader = Arc::new(NumberLoader {
            cur: AtomicU32::new(0),
            failing: AtomicBool::new(false),
        });
        let l = Reloader::reload_periodically(
            CoreContext::test_mock(fb),
            || std::time::Duration::from_millis(5),
            loader.clone(),
        )
        .await
        .unwrap();
        loader.failing.store(true, Relaxed);
        assert_eq!(**l.load(), 0);
        tokio::time::advance(std::time::Duration::from_millis(12)).await;
        l.wait_for_update().await;
        assert_eq!(**l.load(), 0);
        loader.failing.store(false, Relaxed);
        tokio::time::advance(std::time::Duration::from_millis(10)).await;
        l.wait_for_update().await;
        assert!(**l.load() > 0);
    }
}
