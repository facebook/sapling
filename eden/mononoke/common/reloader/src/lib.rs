/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Result};
use arc_swap::ArcSwap;
use async_trait::async_trait;
use cloned::cloned;
use context::CoreContext;
use futures_ext::future::{spawn_controlled, ControlledHandle};
use slog::warn;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Reloader<R> {
    obj: Arc<ArcSwap<R>>,
    _handle: Option<ControlledHandle>,
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
        }
    }
}

#[async_trait]
pub trait Loader<R> {
    async fn load(&mut self) -> Result<Option<R>>;
}

impl<R: 'static + Send + Sync> Reloader<R> {
    pub async fn reload_periodically<
        I: 'static + FnMut() -> Duration + Send,
        L: 'static + Loader<R> + Send + Sync,
    >(
        ctx: CoreContext,
        mut interval_getter: I,
        mut loader: L,
    ) -> Result<Self> {
        let obj = Arc::new(ArcSwap::from_pointee(
            loader
                .load()
                .await?
                .ok_or_else(|| anyhow!("Missing initial object"))?,
        ));
        let handle = spawn_controlled({
            cloned!(obj);
            async move {
                loop {
                    let interval = interval_getter();
                    tokio::time::sleep(interval).await;
                    match loader.load().await {
                        Ok(Some(new)) => obj.store(Arc::new(new)),
                        // Fetch was successful, but there's nothing to reload
                        Ok(None) => {}
                        Err(err) => {
                            warn!(ctx.logger(), "Failed to reload: {:?}", err)
                        }
                    }
                }
            }
        });
        Ok(Self {
            obj,
            _handle: Some(handle),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

    #[test]
    fn test_fixed() {
        let l = Reloader::fixed(12);
        assert_eq!(**l.load(), 12);
        assert_eq!(**l.load(), 12);
    }

    #[fbinit::test]
    async fn test_reload(fb: FacebookInit) {
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
            || std::time::Duration::from_millis(20),
            loader,
        )
        .await
        .unwrap();

        assert_eq!(**l.load(), 0);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(**l.load() > 0);
    }

    #[fbinit::test]
    async fn test_reload_fail_then_succeed(fb: FacebookInit) {
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
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert_eq!(**l.load(), 0);
        loader.failing.store(false, Relaxed);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert!(**l.load() > 0);
    }
}
