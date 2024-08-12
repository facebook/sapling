/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::Weak;

type CallbackAny = dyn (Fn(&dyn Any) -> anyhow::Result<()>) + Send + Sync + 'static;
type Table = HashMap<String, Vec<Weak<CallbackAny>>>;
type RwTable = RwLock<Table>;

/// Registry for callbacks (subscribe) and ability to trigger them (publish).
///
/// Similar to `EventEmitter` from nodejs.
#[derive(Default)]
pub struct PubSub {
    /// Main state.
    table: RwTable,
    /// Total callbacks registered in the table.
    total: AtomicUsize,
    /// Dead (strong ref count = 0) callbacks.
    /// Once exceeding a threadshold, trigger a cleanup.
    dead: Arc<AtomicUsize>,
}

impl PubSub {
    /// Subscribe to events of the `name` with a callback.
    /// When the event happens, the callback will be called.
    /// Dropping the return value cancels the subscription.
    ///
    /// The callback takes an `Any` and needs to handle downcast
    /// and match the publish() type manually.
    pub fn subscribe(
        &self,
        name: impl ToString,
        func: impl (Fn(&dyn Any) -> anyhow::Result<()>) + Send + Sync + 'static,
    ) -> SubscribedHandle {
        self.subscribe_arc(name.to_string(), Arc::new(func))
    }

    /// Publish an event with `name`.
    /// Subscribed callbacks will be called in registration order.
    /// Returns Error if a callback fails, and skips the rest of the callbacks.
    /// Returns Ok if no callback fails.
    pub fn publish(&self, name: &str, value: &dyn Any) -> anyhow::Result<()> {
        self.maybe_cleanup();
        tracing::debug!(name = name, "publish");
        let table = self.table.read().unwrap();
        let callbacks = match table.get(name) {
            None => return Ok(()),
            Some(v) => v,
        };
        for callback in callbacks {
            let callback = match callback.upgrade() {
                None => continue,
                Some(v) => v,
            };
            (callback)(value)?;
        }
        Ok(())
    }

    // de-monomorphizated version of `subscribe`.
    fn subscribe_arc(&self, name: String, func: Arc<CallbackAny>) -> SubscribedHandle {
        self.maybe_cleanup();
        tracing::debug!(name = name, "subscribe");
        let mut table = self.table.write().unwrap();
        table.entry(name).or_default().push(Arc::downgrade(&func));
        self.total.fetch_add(1, Ordering::Release);
        SubscribedHandle {
            dead: Arc::downgrade(&self.dead),
            _arc: func,
        }
    }

    /// Clean up dead callbacks to keep them below 25%.
    fn maybe_cleanup(&self) {
        let dead = self.dead.load(Ordering::Acquire);
        if dead == 0 {
            return;
        }
        let total = self.total.load(Ordering::Acquire);
        if dead <= total / 4 {
            return;
        }

        let mut total_removed = 0usize;
        let mut table = self.table.write().unwrap();
        table.retain(|name, callbacks| {
            let before_len = callbacks.len();
            callbacks.retain(|v| v.upgrade().is_some());
            let after_len = callbacks.len();
            let removed = before_len - after_len;
            if removed > 0 {
                tracing::trace!(
                    name = name,
                    count = removed,
                    remaining = after_len,
                    "cleanup"
                );
            }
            total_removed += removed;
            !callbacks.is_empty()
        });
        self.dead.fetch_sub(total_removed, Ordering::Release);
        self.total.fetch_sub(total_removed, Ordering::Release);

        tracing::debug!(
            dropped = dead,
            subscribed = total,
            removed = total_removed,
            "cleanup"
        );
    }
}

/// Return value of `subscribe`. Dropping it removes the subscription.
pub struct SubscribedHandle {
    // Keep the callback alive.
    _arc: Arc<CallbackAny>,
    // Used by `drop` to update the `dead` count.
    dead: Weak<AtomicUsize>,
}

impl Drop for SubscribedHandle {
    fn drop(&mut self) {
        if let Some(dropped) = self.dead.upgrade() {
            dropped.fetch_add(1, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::SeqCst;

    use super::*;

    #[test]
    fn test_publish_before_subscribe() {
        let p = PubSub::default();

        assert!(p.publish("a1", &1usize).is_ok());
        assert_eq!(p.total(), 0);
        assert_eq!(p.dead(), 0);

        static S1_SUM: AtomicUsize = AtomicUsize::new(0);
        let _s1 = p.subscribe("a1", |x: &dyn Any| acc(&S1_SUM, x));

        // missed the "publish()" because "subscribe()" happened afterwards
        assert_eq!(S1_SUM.load(SeqCst), 0);

        assert!(p.publish("a1", &2usize).is_ok());
        assert_eq!(S1_SUM.load(SeqCst), 2);

        assert_eq!(p.total(), 1);
        assert_eq!(p.dead(), 0);
    }

    #[test]
    fn test_subscribers_on_different_events() {
        let p = PubSub::default();

        static S1_SUM: AtomicUsize = AtomicUsize::new(0);
        static S2_SUM: AtomicUsize = AtomicUsize::new(0);
        let s1 = p.subscribe("a1", |x: &dyn Any| acc(&S1_SUM, x));
        let s2 = p.subscribe("a2", |x: &dyn Any| acc(&S2_SUM, x));

        assert_eq!(p.total(), 2);
        assert_eq!(p.dead(), 0);

        assert!(p.publish("a3", &10usize).is_ok());
        assert_eq!(S1_SUM.load(SeqCst), 0);
        assert_eq!(S2_SUM.load(SeqCst), 0);

        assert!(p.publish("a1", &1usize).is_ok());
        assert!(p.publish("a2", &2usize).is_ok());
        assert_eq!(S1_SUM.load(SeqCst), 1);
        assert_eq!(S2_SUM.load(SeqCst), 2);

        assert!(p.publish("a1", &3usize).is_ok());
        assert!(p.publish("a2", &4usize).is_ok());
        assert_eq!(S1_SUM.load(SeqCst), 4);
        assert_eq!(S2_SUM.load(SeqCst), 6);

        // Drop subscribers, publish() succeed but callbacks are not called.
        drop(s1);
        drop(s2);
        assert!(p.publish("a1", &5usize).is_ok());
        assert!(p.publish("a2", &6usize).is_ok());
        assert_eq!(S1_SUM.load(SeqCst), 4);
        assert_eq!(S2_SUM.load(SeqCst), 6);

        assert_eq!(p.total(), 0);
        assert_eq!(p.dead(), 0);
    }

    #[test]
    fn test_subscribers_on_same_event() {
        let p = PubSub::default();

        static S1_SUM: AtomicUsize = AtomicUsize::new(0);
        static S2_SUM: AtomicUsize = AtomicUsize::new(0);
        let s1 = p.subscribe("b1", |x: &dyn Any| acc(&S1_SUM, x));
        let s2 = p.subscribe("b1", |x: &dyn Any| acc(&S2_SUM, x));

        assert_eq!(p.total(), 2);
        assert_eq!(p.dead(), 0);

        assert!(p.publish("b1", &1usize).is_ok());
        assert!(p.publish("b1", &2usize).is_ok());
        assert!(p.publish("b2", &3usize).is_ok());
        assert_eq!(S1_SUM.load(SeqCst), 3);
        assert_eq!(S2_SUM.load(SeqCst), 3);

        drop(s1);
        assert_eq!(p.total(), 2);
        assert_eq!(p.dead(), 1);

        assert!(p.publish("b1", &5usize).is_ok());
        assert_eq!(S1_SUM.load(SeqCst), 3);
        assert_eq!(S2_SUM.load(SeqCst), 8);

        assert_eq!(p.total(), 1);
        assert_eq!(p.dead(), 0);

        drop(s2);
        assert!(p.publish("b1", &6usize).is_ok());
        assert_eq!(S1_SUM.load(SeqCst), 3);
        assert_eq!(S2_SUM.load(SeqCst), 8);

        assert_eq!(p.total(), 0);
        assert_eq!(p.dead(), 0);
    }

    #[test]
    fn test_drop_subscriber_inside_subscriber_callback_without_deadklock() {
        let p = PubSub::default();

        static S1_SUM: AtomicUsize = AtomicUsize::new(0);
        static S2_SUM: AtomicUsize = AtomicUsize::new(0);
        let s1 = p.subscribe("c1", |x: &dyn Any| acc(&S1_SUM, x));
        let s1 = RwLock::new(Some(s1));
        let _s2 = p.subscribe("c1", move |x: &dyn Any| {
            acc(&S2_SUM, x)?;
            let _ = s1.write().unwrap().take();
            Ok(())
        });

        // Should not deadlock when dropping a subscriber inside another subscriber callback.
        assert!(p.publish("c1", &1usize).is_ok());
        assert_eq!(S1_SUM.load(SeqCst), 1);
        assert_eq!(S2_SUM.load(SeqCst), 1);

        // Subscriber `s1` was dropped.
        assert!(p.publish("c1", &2usize).is_ok());
        assert_eq!(S1_SUM.load(SeqCst), 1);
        assert_eq!(S2_SUM.load(SeqCst), 3);
    }

    #[test]
    fn test_drop_subscriber_drops_closure_immediately() {
        let p = PubSub::default();

        let state = Arc::new(());
        let weak = Arc::downgrade(&state);
        let s1 = p.subscribe("d1", move |_x: &dyn Any| Ok(*state.clone()));
        let s2 = p.subscribe("d2", |_x: &dyn Any| Ok(()));

        assert!(weak.upgrade().is_some());

        // drop(s1) drops the closure (state) immediately, but the dead weakref in `table` stays a
        // bit longer.
        drop(s1);
        assert!(weak.upgrade().is_none());
        assert_eq!(p.total(), 2);
        assert_eq!(p.dead(), 1);

        drop(s2);
        assert!(p.publish("d3", &2usize).is_ok());
        assert_eq!(p.total(), 0);
        assert_eq!(p.dead(), 0);
    }

    impl PubSub {
        fn total(&self) -> usize {
            let total = self.total.load(SeqCst);
            let total2 = self
                .table
                .read()
                .unwrap()
                .values()
                .map(|v| {
                    let len = v.len();
                    assert!(len > 0);
                    len
                })
                .sum();
            assert_eq!(total, total2);
            total
        }

        fn dead(&self) -> usize {
            self.dead.load(SeqCst)
        }
    }

    fn acc(sum: &AtomicUsize, v: &dyn Any) -> anyhow::Result<()> {
        sum.fetch_add(*v.downcast_ref::<usize>().unwrap(), SeqCst);
        Ok(())
    }
}
